use gpui_kit::Context;

use crate::app_state::AppState;
use crate::emoticons;
use crate::rows::reaction_glyph;

pub fn quick_reaction_glyphs(reaction_ids: &[String], skin_tone: u8) -> Vec<String> {
    reaction_ids
        .iter()
        .filter_map(|reaction_id| chatsvc::reaction_glyph_for_key(reaction_id))
        .map(|glyph| emoticons::with_tone(&glyph, skin_tone))
        .collect()
}

/// The most used reactions first, the account defaults fill the remaining slots.
pub fn quick_reactions_with_usage(
    most_used: Vec<String>,
    defaults: Vec<String>,
    skin_tone: u8,
) -> Vec<String> {
    let count = if defaults.is_empty() {
        teams_core::TEAMS_DEFAULT_REACTIONS.len()
    } else {
        defaults.len()
    };
    let mut quick: Vec<String> = Vec::with_capacity(count);
    let toned_used = most_used
        .into_iter()
        .map(|glyph| emoticons::with_tone(&glyph, skin_tone));
    for glyph in toned_used.chain(defaults) {
        if quick.len() == count {
            break;
        }
        let base = reaction_glyph(&emoticons::without_tone(&glyph));
        if !quick
            .iter()
            .any(|known| reaction_glyph(&emoticons::without_tone(known)) == base)
        {
            quick.push(glyph);
        }
    }
    quick
}

pub fn reaction_id_for_glyph(glyph: &str) -> Option<String> {
    chatsvc::reaction_key(&emoticons::without_tone(glyph))
}

pub fn slots_for(reaction_ids: &[String]) -> Vec<Option<String>> {
    let length = reaction_ids.len().max(teams_core::DEFAULT_REACTION_SLOTS);
    (0..length)
        .map(|index| reaction_ids.get(index).cloned())
        .collect()
}

/// Teams' replace rule: an id already in another slot leaves that slot empty.
pub fn place_in_slot(slots: &mut [Option<String>], index: usize, reaction_id: String) {
    for slot in slots.iter_mut() {
        if slot.as_deref() == Some(reaction_id.as_str()) {
            *slot = None;
        }
    }
    if let Some(slot) = slots.get_mut(index) {
        *slot = Some(reaction_id);
    }
}

pub fn filled(slots: &[Option<String>]) -> Vec<String> {
    slots.iter().flatten().cloned().collect()
}

impl AppState {
    pub fn default_reactions(&self) -> Vec<String> {
        teams_core::default_reactions(&self.store)
    }

    pub fn quick_reactions(&self, most_used: Vec<String>) -> Vec<String> {
        let skin_tone = teams_core::skin_tone(&self.store);
        quick_reactions_with_usage(
            most_used,
            quick_reaction_glyphs(&self.default_reactions(), skin_tone),
            skin_tone,
        )
    }

    pub fn set_default_reactions(&mut self, reaction_ids: Vec<String>, cx: &mut Context<Self>) {
        let Some(engine_or_demo) = self.chat_action_engine(cx) else {
            return;
        };
        let previous = self.default_reactions();
        let _ = teams_core::store_account_default_reactions(&self.store, Some(&reaction_ids));
        cx.notify();
        let Some(engine) = engine_or_demo else {
            return;
        };
        self.run_chat_action(
            "Customising default reactions",
            async move { engine.set_default_reactions(&reaction_ids).await },
            move |state, cx| {
                let _ = teams_core::store_account_default_reactions(&state.store, Some(&previous));
                cx.notify();
            },
            |_, _| {},
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn most_used_come_first_and_defaults_fill_the_rest() {
        let defaults = quick_reaction_glyphs(&ids(&teams_core::TEAMS_DEFAULT_REACTIONS), 0);
        let quick = quick_reactions_with_usage(
            vec!["\u{1F680}".to_owned(), "\u{2764}\u{FE0F}".to_owned()],
            defaults.clone(),
            0,
        );
        assert_eq!(quick.len(), defaults.len());
        assert_eq!(quick[0], "\u{1F680}");
        assert_eq!(quick[1], "\u{2764}\u{FE0F}");
        assert_eq!(quick[2], defaults[0]);
        assert_eq!(
            quick.iter().filter(|glyph| reaction_glyph(glyph) == "\u{2764}").count(),
            1
        );
    }

    #[test]
    fn used_reactions_take_the_account_tone() {
        let defaults = quick_reaction_glyphs(&ids(&teams_core::TEAMS_DEFAULT_REACTIONS), 2);
        let quick = quick_reactions_with_usage(vec!["\u{1F44D}".to_owned()], defaults, 2);
        assert_eq!(quick[0], "\u{1F44D}\u{1F3FC}");
        assert_eq!(quick.iter().filter(|glyph| glyph.starts_with('\u{1F44D}')).count(), 1);
    }

    #[test]
    fn teams_defaults_show_as_glyphs_and_like_takes_the_tone() {
        let defaults = ids(&teams_core::TEAMS_DEFAULT_REACTIONS);
        assert_eq!(
            quick_reaction_glyphs(&defaults, 0),
            ["\u{1F44D}", "\u{2764}", "\u{1F606}", "\u{1F62E}"]
        );
        assert_eq!(quick_reaction_glyphs(&defaults, 2)[0], "\u{1F44D}\u{1F3FC}");
        assert_eq!(
            chatsvc::reaction_key(&quick_reaction_glyphs(&defaults, 2)[0]).as_deref(),
            Some("yes-tone2")
        );
    }

    #[test]
    fn unknown_ids_are_left_out() {
        assert_eq!(
            quick_reaction_glyphs(&ids(&["heart", "custom-gone"]), 0),
            ["\u{2764}"]
        );
    }

    #[test]
    fn picked_glyphs_store_the_untoned_id() {
        assert_eq!(
            reaction_id_for_glyph("\u{1F44D}\u{1F3FC}").as_deref(),
            Some("like")
        );
        assert_eq!(
            reaction_id_for_glyph("\u{2764}\u{FE0F}").as_deref(),
            Some("heart")
        );
    }

    #[test]
    fn there_are_always_five_slots_and_a_repeat_moves() {
        let mut slots = slots_for(&ids(&["like", "heart"]));
        assert_eq!(slots.len(), 5);
        place_in_slot(&mut slots, 3, "like".to_owned());
        assert_eq!(filled(&slots), ["heart", "like"]);
        assert_eq!(slots[0], None);
        assert_eq!(slots_for(&ids(&["a", "b", "c", "d", "e", "f"])).len(), 6);
    }
}
