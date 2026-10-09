use std::sync::OnceLock;

const CATALOG: &str = include_str!("../assets/stickers.tsv");
const IMAGE_BASE: &str = "https://statics.teams.cdn.office.net/evergreen-assets/stickerassets";
const IMAGE_VERSION: u32 = 5;
const POPULAR_IDS: [&str; 6] = [
    "Clippy_HelloThere",
    "OfficeDrama_HighFive",
    "WordArt_Awesome",
    "Octocorn_Coffee",
    "Teamsquatch_Yes",
    "Bee_Angry",
];

#[derive(Debug, PartialEq, Eq)]
pub struct Sticker {
    pub id: &'static str,
    pub category: &'static str,
    pub file: &'static str,
    pub name: &'static str,
}

impl Sticker {
    pub fn url(&self) -> String {
        format!(
            "{IMAGE_BASE}/{}-250x250/{}?v={IMAGE_VERSION}",
            self.category.to_lowercase(),
            self.file
        )
    }
}

fn catalog() -> &'static [Sticker] {
    static PARSED: OnceLock<Vec<Sticker>> = OnceLock::new();
    PARSED.get_or_init(|| {
        CATALOG
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                Some(Sticker {
                    id: fields.next()?,
                    category: fields.next()?,
                    file: fields.next()?,
                    name: fields.next()?,
                })
            })
            .collect()
    })
}

pub fn popular() -> Vec<&'static Sticker> {
    POPULAR_IDS
        .iter()
        .filter_map(|id| catalog().iter().find(|sticker| sticker.id == *id))
        .collect()
}

pub fn categories() -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    for sticker in catalog() {
        if !found.contains(&sticker.category) {
            found.push(sticker.category);
        }
    }
    found
}

pub fn in_category(category: &str) -> Vec<&'static Sticker> {
    catalog()
        .iter()
        .filter(|sticker| sticker.category == category)
        .collect()
}

/// `OfficeDrama` becomes "Office Drama", `CatsInSuits` becomes "Cats in Suits", `BMac` stays.
pub fn category_label(category: &str) -> String {
    let mut label = String::new();
    let mut previous: Option<char> = None;
    for character in category.chars() {
        if character.is_uppercase() && previous.is_some_and(char::is_lowercase) {
            label.push(' ');
        }
        label.push(character);
        previous = Some(character);
    }
    label
        .split(' ')
        .enumerate()
        .map(|(index, word)| match word {
            "In" | "Of" | "And" if index > 0 => word.to_lowercase(),
            other => other.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn search(query: &str) -> Vec<&'static Sticker> {
    let query = query.trim().to_lowercase();
    catalog()
        .iter()
        .filter(|sticker| {
            [sticker.name, sticker.id, sticker.category]
                .iter()
                .any(|field| field.to_lowercase().contains(&query))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_parses_every_row() {
        assert_eq!(catalog().len(), CATALOG.lines().count());
        assert!(catalog().iter().all(|sticker| !sticker.file.is_empty()));
    }

    #[test]
    fn urls_use_the_lowercase_category_folder() {
        let sticker = catalog()
            .iter()
            .find(|sticker| sticker.id == "Clippy_HelloThere")
            .unwrap();
        assert_eq!(
            sticker.url(),
            "https://statics.teams.cdn.office.net/evergreen-assets/stickerassets/clippy-250x250/Clippy_HelloThere.gif?v=5"
        );
    }

    #[test]
    fn categories_keep_the_catalog_order() {
        let found = categories();
        assert_eq!(found.first(), Some(&"Clippy"));
        assert_eq!(found.len(), 18);
    }

    #[test]
    fn labels_split_camel_case() {
        assert_eq!(category_label("OfficeDrama"), "Office Drama");
        assert_eq!(category_label("CatsInSuits"), "Cats in Suits");
        assert_eq!(category_label("BMac"), "BMac");
        assert_eq!(category_label("Clippy"), "Clippy");
    }

    #[test]
    fn popular_follows_the_given_order() {
        let ids: Vec<&str> = popular().iter().map(|sticker| sticker.id).collect();
        assert_eq!(ids[0], "Clippy_HelloThere");
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn search_ignores_case_and_covers_name_id_and_category() {
        assert!(
            search("hello there")
                .iter()
                .any(|sticker| sticker.id == "Clippy_HelloThere")
        );
        assert!(
            search("OCTOCORN_COFFEE")
                .iter()
                .any(|sticker| sticker.id == "Octocorn_Coffee")
        );
        assert!(
            search("wordart")
                .iter()
                .all(|sticker| sticker.category == "WordArt"
                    || sticker.name.to_lowercase().contains("wordart")
                    || sticker.id.to_lowercase().contains("wordart"))
        );
        assert!(search("zzzzqqq").is_empty());
    }
}
