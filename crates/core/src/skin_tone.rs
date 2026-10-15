use store::Store;

use crate::error::Result;

const META_ACCOUNT_SKIN_TONE: &str = "skin_tone_account";
const META_PICKED_SKIN_TONE: &str = "skin_tone_picked";

fn read(store: &Store, key: &str) -> Option<u8> {
    store.meta(key).ok().flatten()?.parse().ok()
}

pub fn store_account_skin_tone(store: &Store, tone: u8) -> Result<()> {
    Ok(store.set_meta(META_ACCOUNT_SKIN_TONE, &tone.to_string())?)
}

pub fn store_picked_skin_tone(store: &Store, tone: u8) -> Result<()> {
    Ok(store.set_meta(META_PICKED_SKIN_TONE, &tone.to_string())?)
}

/// 0 is the default yellow, 1 to 5 light to dark. A tone picked in this app beats the
/// account setting, which the Teams web client only reads and never writes.
pub fn skin_tone(store: &Store) -> u8 {
    read(store, META_PICKED_SKIN_TONE)
        .or_else(|| read(store, META_ACCOUNT_SKIN_TONE))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_tone_beats_the_account_tone() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(skin_tone(&store), 0);
        store_account_skin_tone(&store, 2).unwrap();
        assert_eq!(skin_tone(&store), 2);
        store_picked_skin_tone(&store, 4).unwrap();
        assert_eq!(skin_tone(&store), 4);
    }
}
