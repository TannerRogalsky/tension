pub mod viewer;

use serde::{Deserialize, Serialize};

pub const ENDPOINT_WS: &'static str = "socket";
pub const ENDPOINT_CREATE_ROOM: &'static str = "create";
pub const ENDPOINT_JOIN_ROOM: &'static str = "join";

#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
pub struct RoomJoinInfo {
    pub room_id: RoomID,
    pub player_name: PlayerName,
}

pub type PlayerName = String;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerID(u64);

impl PlayerID {
    pub fn gen<R: rand::Rng>(rng: &mut R) -> Self {
        Self(rng.gen())
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CustomMessage {
    StartGame,
    Click(f32, f32),
    AssignClick(PlayerID, u32),
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
}
