type ArcRw<T> = std::sync::Arc<tokio::sync::RwLock<T>>;
type EventSink = tokio::sync::mpsc::UnboundedSender<super::CustomMessage>;

// What data is in User cause we don't have their name when this would be created.
struct User {
    player: shared::Player,
    rooms: Vec<shared::RoomID>,
}

struct Room {
    id: shared::RoomID,
    users: Vec<shared::PlayerID>,
}

#[derive(Clone)]
pub struct State {
    event_sink: EventSink,
    users: ArcRw<std::collections::HashMap<shared::PlayerID, User>>,
    rooms: ArcRw<std::collections::HashMap<shared::RoomID, Room>>,
}

impl State {
    pub fn new(event_sink: EventSink) -> Self {
        Self {
            event_sink,
            users: Default::default(),
            rooms: Default::default(),
        }
    }

    pub async fn create_room(&mut self, player: shared::Player) -> shared::RoomID {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::from_entropy();
        let room_id = shared::RoomID::new(&mut rng);
        drop(rng);
        let player_id = player.id;

        let room = Room {
            id: room_id,
            users: vec![player_id],
        };
        let user = User {
            player: player.clone(),
            rooms: vec![room_id],
        };

        self.rooms.write().await.insert(room_id, room);
        self.users.write().await.insert(player_id, user);

        let message = shared::Message::joined(room_id, player);
        if let Err(err) = self.event_sink.send(message) {
            log::error!("{}", err);
        }

        room_id
    }

    pub async fn join_room(
        &mut self,
        player: shared::Player,
        room_id: shared::RoomID,
    ) -> shared::RoomState {
        let player_id = player.id;
        self.users.write().await.insert(
            player_id,
            User {
                player: player.clone(),
                rooms: vec![room_id],
            },
        );

        let players = if let Some(room) = self.rooms.write().await.get_mut(&room_id) {
            room.users.push(player_id);

            let users = self.users.read().await;
            room.users
                .iter()
                .filter_map(|user_id| users.get(user_id).map(|user| user.player.clone()))
                .collect::<Vec<_>>()
        } else {
            vec![]
        };

        if let Err(err) = self
            .event_sink
            .send(shared::Message::joined(room_id, player))
        {
            log::error!("{}", err);
        }

        shared::RoomState {
            id: room_id,
            players,
        }
    }

    pub async fn leave_room(&mut self, player_id: shared::PlayerID, room_id: shared::RoomID) {
        if let Some(room) = self.rooms.write().await.get_mut(&room_id) {
            if let Some(position) = room.users.iter().position(|p| p == &player_id) {
                room.users.remove(position);
            }
        }

        if let Err(err) = self
            .event_sink
            .send(shared::Message::left(room_id, player_id))
        {
            log::error!("{}", err);
        }
    }

    pub async fn disconnect(&mut self, player_id: shared::PlayerID) {
        let user = self.users.write().await.remove(&player_id);
        if let Some(user) = user {
            for room_id in user.rooms {
                self.leave_room(player_id, room_id).await;
            }
        }
    }

    pub async fn players(&self, room_id: shared::RoomID) -> Option<Vec<shared::PlayerID>> {
        self.rooms
            .read()
            .await
            .get(&room_id)
            .map(|room| room.users.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[tokio::test]
    async fn basic() {
        let (sx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut state = State::new(sx);

        let p1 = shared::Player {
            id: shared::PlayerID::from_str("0").unwrap(),
            name: "Alice".to_string(),
        };
        let p2 = shared::Player {
            id: shared::PlayerID::from_str("1").unwrap(),
            name: "Bob".to_string(),
        };

        let room_id = state.create_room(p1.clone()).await;
        assert_eq!(
            rx.recv().await,
            Some(shared::Message::joined(room_id, p1.clone()))
        );

        let room_state = state.join_room(p2.clone(), room_id).await;
        assert_eq!(
            room_state,
            shared::RoomState {
                id: room_id,
                players: vec![p1.clone(), p2.clone()]
            }
        );
        assert_eq!(
            rx.recv().await,
            Some(shared::Message::joined(room_id, p2.clone()))
        );

        state.disconnect(p1.id).await;
        assert_eq!(rx.recv().await, Some(shared::Message::left(room_id, p1.id)));
        assert_eq!(state.players(room_id).await, Some(vec![p2.id]));
    }
}
