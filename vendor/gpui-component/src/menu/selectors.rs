pub const ITEMS_SELECTOR: &str = "popup-menu/items";

pub fn item_selector(index: usize) -> String {
    format!("popup-menu/item-{index}")
}

pub fn icon_selector(index: usize) -> String {
    format!("popup-menu/item-{index}/icon")
}

pub fn label_selector(index: usize) -> String {
    format!("popup-menu/item-{index}/label")
}
