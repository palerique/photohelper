pub fn nima_score_to_rating_and_tier(score: f32) -> (i32, &'static str) {
    if score < 4.0 {
        (1, "discard")
    } else if score < 5.5 {
        (2, "poor")
    } else if score < 7.0 {
        (3, "fair")
    } else if score < 8.5 {
        (4, "good")
    } else {
        (5, "excellent")
    }
}

pub fn format_nima_score_label(score: f32) -> String {
    format!("{score:05.2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_nima_score_label() {
        assert_eq!(format_nima_score_label(9.5), "09.50");
        assert_eq!(format_nima_score_label(10.0), "10.00");
        assert_eq!(format_nima_score_label(3.1425), "03.14");
    }
}
