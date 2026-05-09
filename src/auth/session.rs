use rand::RngCore;

pub const COOKIE_NAME: &str = "vps_monitor_session";

pub fn new_token() -> String {
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}
