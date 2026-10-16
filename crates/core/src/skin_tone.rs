use store::Store;

use crate::error::Result;

const META_ACCOUNT_SKIN_TONE: &str = "skin_tone_account";

pub fn store_account_skin_tone(store: &Store, tone: u8) -> Result<()> {
    Ok(store.set_meta(META_ACCOUNT_SKIN_TONE, &tone.to_string())?)
}

/// The account setting `preferredSkinToneIndex`: 0 is the default yellow, 1 to 5 light to dark.
pub fn skin_tone(store: &Store) -> u8 {
    store
        .meta(META_ACCOUNT_SKIN_TONE)
        .ok()
        .flatten()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_tone_is_stored_and_read_back() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(skin_tone(&store), 0);
        store_account_skin_tone(&store, 2).unwrap();
        assert_eq!(skin_tone(&store), 2);
    }
}
