#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndKind {
    Normal,
    Cancelled,
    Declined,
    NoAnswer,
    Unavailable,
    AnsweredElsewhere,
    Forwarded,
    MediaError,
    Other,
}

const SUB_CODE_DECLINED: i64 = 10603;
const SUB_CODES_NO_ANSWER: [i64; 2] = [10408, 10486];

pub fn classify_end(code: i64, sub_code: i64) -> EndKind {
    match code {
        0 => EndKind::Normal,
        487 => EndKind::Cancelled,
        603 => EndKind::Declined,
        408 => EndKind::NoAnswer,
        480 => EndKind::Unavailable,
        450 => EndKind::AnsweredElsewhere,
        451 | 452 => EndKind::Forwarded,
        410 => EndKind::MediaError,
        _ if sub_code == SUB_CODE_DECLINED => EndKind::Declined,
        _ if SUB_CODES_NO_ANSWER.contains(&sub_code) => EndKind::NoAnswer,
        _ => EndKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_to_their_meaning() {
        assert_eq!(classify_end(0, 5001), EndKind::Normal);
        assert_eq!(classify_end(487, 0), EndKind::Cancelled);
        assert_eq!(classify_end(603, 0), EndKind::Declined);
        assert_eq!(classify_end(408, 0), EndKind::NoAnswer);
        assert_eq!(classify_end(480, 0), EndKind::Unavailable);
        assert_eq!(classify_end(450, 0), EndKind::AnsweredElsewhere);
        assert_eq!(classify_end(451, 0), EndKind::Forwarded);
        assert_eq!(classify_end(452, 0), EndKind::Forwarded);
        assert_eq!(classify_end(410, 0), EndKind::MediaError);
    }

    #[test]
    fn unknown_codes_fall_back_to_the_sub_code() {
        assert_eq!(classify_end(500, 10603), EndKind::Declined);
        assert_eq!(classify_end(500, 10408), EndKind::NoAnswer);
        assert_eq!(classify_end(500, 10486), EndKind::NoAnswer);
        assert_eq!(classify_end(500, 10600), EndKind::Other);
        assert_eq!(classify_end(500, 0), EndKind::Other);
    }
}
