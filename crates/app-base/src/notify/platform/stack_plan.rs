use super::NativeHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackPlan {
    Keep,
    Rebuild { after: Option<NativeHandle> },
}

pub fn stack_plan(above: &[NativeHandle], group: &[NativeHandle]) -> StackPlan {
    let border_count = above
        .iter()
        .take_while(|handle| group.contains(handle))
        .count();
    let covers_group = group
        .iter()
        .all(|handle| above[..border_count].contains(handle));
    if covers_group {
        StackPlan::Keep
    } else {
        StackPlan::Rebuild {
            after: above.get(border_count).copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUP: [NativeHandle; 4] = [1, 2, 3, 4];

    #[test]
    fn rotated_group_directly_above_is_kept() {
        assert_eq!(stack_plan(&[3, 1, 4, 2, 99], &GROUP), StackPlan::Keep);
        assert_eq!(stack_plan(&[1, 2, 3, 4], &GROUP), StackPlan::Keep);
    }

    #[test]
    fn foreign_window_between_rebuilds_after_it() {
        assert_eq!(
            stack_plan(&[1, 2, 99], &GROUP),
            StackPlan::Rebuild { after: Some(99) }
        );
        assert_eq!(
            stack_plan(&[99, 1, 2, 3, 4], &GROUP),
            StackPlan::Rebuild { after: Some(99) }
        );
    }

    #[test]
    fn nothing_above_rebuilds_with_band_rule() {
        assert_eq!(stack_plan(&[], &GROUP), StackPlan::Rebuild { after: None });
    }

    #[test]
    fn shared_topmost_with_missing_borders_rebuilds_with_band_rule() {
        assert_eq!(
            stack_plan(&[2, 4], &GROUP),
            StackPlan::Rebuild { after: None }
        );
    }

    #[test]
    fn empty_group_is_kept() {
        assert_eq!(stack_plan(&[99], &[]), StackPlan::Keep);
    }
}
