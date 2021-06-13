use crate::{PlayerID as UserID, RoomID};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeType<T> {
    UserJoin(User),
    UserLeave(UserID),
    Custom(T),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange<T> {
    pub target: RoomID,
    pub ty: ChangeType<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserID,
    pub name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoomState {
    pub id: RoomID,
    pub users: Vec<UserID>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Command<T> {
    Disconnect(UserID),
    Custom(RoomID, T),
}

#[cfg(feature = "server")]
pub mod state {
    use super::*;
    use tokio::sync::broadcast as channel;

    pub struct Room<T> {
        pub state: RoomState,
        pub channel: channel::Sender<StateChange<T>>,
    }

    #[derive(Default)]
    pub struct State<T> {
        pub users: std::collections::HashMap<UserID, User>,
        pub rooms: std::collections::HashMap<RoomID, Room<T>>,
    }

    /// When joining a room, is it better to join then sub or sub then join?
    impl<T: std::fmt::Debug + Clone> State<T> {
        pub fn register_user(&mut self, user: User) {
            let user_id = user.id;
            if self.users.insert(user_id, user).is_some() {
                log::error!("Overwrote an existing user @ {:?}!", user_id);
            }
        }

        /// Return initial state and a channel of changes
        pub fn subscribe(
            &self,
            room_id: RoomID,
        ) -> Option<(RoomState, channel::Receiver<StateChange<T>>)> {
            self.rooms
                .get(&room_id)
                .map(|room| (room.state.clone(), room.channel.subscribe()))
        }

        pub fn create_room(&mut self) -> RoomID {
            let mut rng = rand::thread_rng();
            let room_id = crate::RoomID::new(&mut rng);
            let (channel, _) = channel::channel(16);
            self.rooms.insert(
                room_id,
                Room {
                    state: RoomState {
                        id: room_id,
                        users: vec![],
                    },
                    channel,
                },
            );
            room_id
        }

        pub fn join(
            &mut self,
            room_id: RoomID,
            user_id: UserID,
        ) -> Option<Result<usize, channel::error::SendError<StateChange<T>>>> {
            let room = self.rooms.get_mut(&room_id);
            let user = self.users.get(&user_id);
            room.zip(user).map(|(room, user)| {
                room.state.users.push(user.id);
                room.channel.send(StateChange {
                    target: room_id,
                    ty: ChangeType::UserJoin(user.clone()),
                })
            })
        }

        pub fn leave(
            &mut self,
            room_id: RoomID,
            user_id: UserID,
        ) -> Option<Result<usize, channel::error::SendError<StateChange<T>>>> {
            self.rooms.get_mut(&room_id).map(|room| {
                room.state.users.retain(|user| user != &user_id);
                room.channel.send(StateChange {
                    target: room_id,
                    ty: ChangeType::UserLeave(user_id),
                })
            })
        }

        pub fn unregister_user(&mut self, user_id: UserID) {
            self.users.remove(&user_id);
            let to_remove = self
                .rooms
                .iter_mut()
                .filter_map(|(room_id, room)| {
                    let index = room.state.users.iter().position(|user| user == &user_id);
                    if let Some(index) = index {
                        room.state.users.remove(index);
                        let result = room.channel.send(StateChange {
                            target: room.state.id,
                            ty: ChangeType::UserLeave(user_id),
                        });
                        if let Err(err) = result {
                            log::error!("{:?}", err);
                        }
                    }
                    if room.state.users.is_empty() {
                        Some(*room_id)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            for room_id in to_remove {
                self.rooms.remove(&room_id);
            }
        }

        pub fn handle_command(&mut self, cmd: Command<T>) {
            match cmd {
                Command::Disconnect(user_id) => {
                    self.unregister_user(user_id);
                }
                Command::Custom(room_id, payload) => {
                    if let Some(room) = self.rooms.get(&room_id) {
                        let result = room.channel.send(StateChange {
                            target: room.state.id,
                            ty: ChangeType::Custom(payload),
                        });
                        if let Err(err) = result {
                            log::error!("{:?}", err);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(feature = "client")]
pub mod view {
    use super::*;

    #[derive(Debug, Clone, Eq, PartialEq)]
    pub struct Room {
        pub state: RoomState,
    }

    #[derive(Debug, Default)]
    pub struct State {
        pub users: Vec<User>,
        pub rooms: Vec<Room>,
    }

    #[derive(Debug, Default)]
    pub struct View {
        pub state: State,
    }
}

#[cfg(test)]
mod tests {
    use super::{state, view, *};
    use tokio_stream::StreamExt;

    static USER_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    struct UserView<T> {
        user: User,
        view: view::View,
        // represents the server-side websocket sender
        sx: crossbeam_channel::Sender<StateChange<T>>,
        // represents the client-side websocket receiver
        rx: crossbeam_channel::Receiver<StateChange<T>>,
        // represents the client-side websocket sender
        to_server: tokio::sync::mpsc::UnboundedSender<Command<T>>,
    }

    impl<T> UserView<T> {
        pub fn new(name: String) -> (Self, tokio::sync::mpsc::UnboundedReceiver<Command<T>>) {
            let (sx, rx) = crossbeam_channel::unbounded();
            let (to_server, from_client) = tokio::sync::mpsc::unbounded_channel();
            let this = Self {
                user: User {
                    id: UserID(USER_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)),
                    name,
                },
                view: view::View {
                    state: view::State {
                        users: vec![],
                        rooms: vec![],
                    },
                },
                sx,
                rx,
                to_server,
            };
            (this, from_client)
        }

        pub fn update(&mut self) {
            for msg in self.rx.try_iter() {
                let room = self
                    .view
                    .state
                    .rooms
                    .iter_mut()
                    .find(|room| room.state.id == msg.target);
                if let Some(room) = room {
                    match msg.ty {
                        ChangeType::UserJoin(user) => {
                            room.state.users.push(user.id);
                        }
                        ChangeType::UserLeave(user_id) => {
                            room.state.users.retain(|user| user != &user_id);
                        }
                        ChangeType::Custom(_) => {}
                    }
                }
            }
        }
    }

    // in a real use this event would be generated by the socket disconnecting
    impl<T> Drop for UserView<T> {
        fn drop(&mut self) {
            if let Err(err) = self.to_server.send(Command::Disconnect(self.user.id)) {
                log::error!(
                    "Disconnect event for {} failed to send: {}",
                    self.user.id.0,
                    err
                );
            }
        }
    }

    #[tokio::test]
    async fn sync_test() {
        simple_logger::SimpleLogger::new().init().unwrap();

        type CustomMessage = ();

        let (mut user1, from_user1) = UserView::<CustomMessage>::new("Alice".to_string());
        let (mut user2, from_user2) = UserView::<CustomMessage>::new("Bob".to_string());

        let state = std::sync::Arc::new(tokio::sync::RwLock::new(
            state::State::<CustomMessage>::default(),
        ));

        {
            let mut state = state.write().await;
            state.register_user(user1.user.clone());
            state.register_user(user2.user.clone());
        }

        tokio::spawn({
            let state = state.clone();
            async move {
                let streams = std::array::IntoIter::new([from_user1, from_user2]).map(
                    tokio_stream::wrappers::UnboundedReceiverStream::<Command<CustomMessage>>::new,
                );
                let mut all = futures::stream::select_all(streams);
                while let Some(cmd) = all.next().await {
                    state.write().await.handle_command(cmd);
                }
            }
        });

        let room1 = {
            let initial_state = create_room(&mut *state.write().await, &user1).await;
            let room_id = initial_state.id;
            user1.view.state.rooms.push(view::Room {
                state: initial_state,
            });
            room_id
        };

        {
            let initial_state = join_room(&mut *state.write().await, room1, &user2).await;
            user2.view.state.rooms.push(view::Room {
                state: initial_state,
            });
        }

        // just give the async stuff a chance to run
        tokio::time::sleep(std::time::Duration::from_secs(0)).await;

        user1.update();
        user2.update();

        cmp_room_states(&state.read().await.rooms, &user1.view.state.rooms);
        cmp_room_states(&state.read().await.rooms, &user2.view.state.rooms);
        assert_eq!(user1.view.state.rooms, user2.view.state.rooms);

        state.write().await.leave(room1, user1.user.id);

        tokio::time::sleep(std::time::Duration::from_secs(0)).await;
        user1.update();
        user2.update();

        cmp_room_states(&state.read().await.rooms, &user1.view.state.rooms);
        cmp_room_states(&state.read().await.rooms, &user2.view.state.rooms);
        assert_eq!(user1.view.state.rooms, user2.view.state.rooms);

        drop(user2);

        tokio::time::sleep(std::time::Duration::from_secs(0)).await;
        assert_eq!(state.read().await.rooms.len(), 0);
    }

    // roughly analogous to an http request
    async fn create_room<T>(state: &mut state::State<T>, user: &UserView<T>) -> RoomState
    where
        T: std::fmt::Debug + Clone + Send + 'static,
    {
        let room_id = state.create_room();
        state.join(room_id, user.user.clone());
        let (state, channel) = state.subscribe(room_id).unwrap();
        let sx = user.sx.clone();
        let user_id = user.user.id;
        // what's the implication of leaving this running when the view disconnects?
        // is that even a problem in a "real" implementation?
        tokio::spawn(async move {
            let mut channel = tokio_stream::wrappers::BroadcastStream::new(channel);
            while let Some(Ok(msg)) = channel.next().await {
                if let Err(err) = sx.send(msg) {
                    log::error!("Error sending {:?} to {:?}: {}", err.0, user_id, err);
                    break;
                }
            }
        });
        state
    }

    async fn join_room<T>(
        state: &mut state::State<T>,
        room_id: RoomID,
        user: &UserView<T>,
    ) -> RoomState
    where
        T: std::fmt::Debug + Clone + Send + 'static,
    {
        state.join(room_id, user.user.id);
        let (state, channel) = state.subscribe(room_id).unwrap();
        let sx = user.sx.clone();
        let user_id = user.user.id;
        tokio::spawn(async move {
            let mut channel = tokio_stream::wrappers::BroadcastStream::new(channel);
            while let Some(Ok(msg)) = channel.next().await {
                if let Err(err) = sx.send(msg) {
                    log::error!("Error sending {:?} to {:?}: {}", err.0, user_id, err);
                    break;
                }
            }
        });
        state
    }

    fn cmp_room_states<T>(
        lhs: &std::collections::HashMap<RoomID, state::Room<T>>,
        rhs: &[view::Room],
    ) {
        let lhs = lhs.values().map(|room| &room.state).collect::<Vec<_>>();
        let rhs = rhs.iter().map(|room| &room.state).collect::<Vec<_>>();
        assert_eq!(lhs, rhs);
    }
}
