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
