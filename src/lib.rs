pub mod connection;
pub mod packets;
pub mod utils;

pub const DEFAULT_PORT: u16 = 13723;

pub const RSA_BITS: usize = 1024;
pub const ENC_TOK_LEN: usize = 32; // Length of the confirmation token sent by the server
pub const SECRET_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
