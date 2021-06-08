use serde::{Deserialize, Serialize};

type Lookup<K, V> = std::collections::HashMap<K, V>;

pub const ENDPOINT_WS: &'static str = "socket";
pub const ENDPOINT_CREATE_ROOM: &'static str = "create";
pub const ENDPOINT_JOIN_ROOM: &'static str = "join";

pub trait Receiver<T> {
    fn try_recv(&self) -> Result<Message<T>, ()>;
}

pub trait Sender<T> {
    fn send(&self, msg: Message<T>) -> Result<(), ()>;
}

pub struct Client<T, S, R> {
    recv: R,
    send: S,
    rooms: Lookup<RoomID, Room<T>>,
}

impl<T, S, R> Client<T, S, R>
where
    R: Receiver<T>,
    S: Sender<T>,
{
    pub fn new(send: S, recv: R) -> Self {
        Self {
            send,
            recv,
            rooms: Default::default(),
        }
    }

    pub fn get_room(&self, id: &RoomID) -> Option<RoomRead<'_, T, S>> {
        self.rooms.get(id).map(|room| RoomRead {
            room,
            sender: &self.send,
        })
    }

    pub fn update(&mut self) {
        while let Ok(msg) = self.recv.try_recv() {
            if let Some(room) = self.rooms.get_mut(&msg.target) {
                match msg.ty {
                    MessageType::PlayerJoined(player) => {
                        room.state.players.push(player);
                    }
                    MessageType::PlayerLeft(player) => {
                        let players = &mut room.state.players;
                        if let Some(position) = players.iter().position(|p| p.id == player) {
                            players.remove(position);
                        }
                    }
                    MessageType::Custom(v) => room.custom_messages.push_back(v),
                }
            } else if let MessageType::PlayerJoined(player) = msg.ty {
                self.rooms.insert(
                    msg.target,
                    Room {
                        state: RoomState {
                            id: msg.target,
                            players: vec![player],
                        },
                        custom_messages: Default::default(),
                    },
                );
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Message<T> {
    target: RoomID,
    ty: MessageType<T>,
}

impl<T> Message<T> {
    pub fn joined(target: RoomID, player: Player) -> Self {
        Self {
            target,
            ty: MessageType::PlayerJoined(player),
        }
    }

    pub fn left(target: RoomID, player: PlayerID) -> Self {
        Self {
            target,
            ty: MessageType::PlayerLeft(player),
        }
    }

    pub fn target(&self) -> RoomID {
        self.target
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum MessageType<T> {
    PlayerJoined(Player),
    PlayerLeft(PlayerID),
    Custom(T),
}

#[derive(
    Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash, Default, Serialize, Deserialize,
)]
pub struct RoomID([u8; 4]);

impl RoomID {
    const LENGTH: usize = 4;

    pub fn new<R: rand::Rng>(rng: &mut R) -> Self {
        let mut gen = || rng.gen_range(b'A'..=b'Z');
        Self([gen(), gen(), gen(), gen()])
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, thiserror::Error)]
pub enum RoomIDParseError {
    #[error("Room ID must be exactly {} characters long.", RoomID::LENGTH)]
    TooShort,
    #[error("Encountered an invalid character: {0}")]
    UnrecognizedCharacter(char),
}

impl std::str::FromStr for RoomID {
    type Err = RoomIDParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != Self::LENGTH {
            Err(RoomIDParseError::TooShort)
        } else {
            let mut iter = value.chars().map(|c| {
                if c.is_ascii_alphabetic() {
                    Ok(c.to_ascii_uppercase() as u8)
                } else {
                    Err(RoomIDParseError::UnrecognizedCharacter(c))
                }
            });
            Ok(Self([
                iter.next().unwrap()?,
                iter.next().unwrap()?,
                iter.next().unwrap()?,
                iter.next().unwrap()?,
            ]))
        }
    }
}

impl std::fmt::Display for RoomID {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let i = &self.0;
        write!(
            f,
            "{}{}{}{}",
            i[0] as char, i[1] as char, i[2] as char, i[3] as char
        )
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomState {
    pub id: RoomID,
    pub players: Vec<Player>,
}

#[derive(Debug)]
pub struct Room<T> {
    pub state: RoomState,
    pub custom_messages: std::collections::VecDeque<T>,
}

impl<T> Room<T> {
    pub fn players(&self) -> &[Player] {
        &self.state.players
    }
}

#[derive(Debug, Copy, Clone)]
pub struct RoomRead<'a, T, S> {
    room: &'a Room<T>,
    sender: &'a S,
}

impl<T, S> std::ops::Deref for RoomRead<'_, T, S> {
    type Target = Room<T>;

    fn deref(&self) -> &Self::Target {
        self.room
    }
}

impl<T, S> RoomRead<'_, T, S>
where
    S: Sender<T>,
{
    pub fn send(&self, msg: T) -> Result<(), ()> {
        self.sender.send(Message {
            target: self.room.state.id,
            ty: MessageType::Custom(msg),
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct RoomJoinInfo {
    pub room_id: RoomID,
    pub player_name: PlayerName,
}

pub type PlayerName = String;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerID(u64);

impl std::str::FromStr for PlayerID {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse::<u64>()?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerID,
    pub name: PlayerName,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_id_test() {
        let mut rng = rand::rngs::mock::StepRng::new(u64::MAX / 1000, u64::MAX / 100);
        let room = RoomID::new(&mut rng);
        assert_eq!(room.to_string(), String::from("HGFE"));
    }

    #[test]
    fn cross_beam_test() {
        impl<T> Receiver<T> for crossbeam_channel::Receiver<Message<T>> {
            fn try_recv(&self) -> Result<Message<T>, ()> {
                self.try_recv().map_err(|_| ())
            }
        }

        impl<T> Sender<T> for crossbeam_channel::Sender<Message<T>> {
            fn send(&self, msg: Message<T>) -> Result<(), ()> {
                self.send(msg).map_err(|_| ())
            }
        }

        let (sx, rx) = crossbeam_channel::unbounded();
        let mut client = Client::new(sx, rx);

        let room1 = client.create_room();
        let room1_handle = room1.id;
        room1.send(1).expect("failed send");

        assert_eq!(
            client.recv.try_recv(),
            Ok(Message {
                target: room1_handle,
                ty: MessageType::Custom(1)
            })
        );
    }
}
