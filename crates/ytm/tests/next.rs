//! Up-next queue against a captured `next` response for "kJQP7kiw5Fk" (Despacito).

use tubeamp_ytm::parse_next;

const NEXT: &str = include_str!("fixtures/next.json");

#[test]
fn up_next_has_at_least_five_playable_tracks() {
    let tracks = parse_next(NEXT);
    assert!(tracks.len() >= 5, "only {} tracks", tracks.len());
    for t in &tracks {
        assert_eq!(t.video_id.len(), 11, "video id {:?}", t.video_id);
        assert!(!t.title.is_empty());
    }
}

#[test]
fn the_seed_video_leads_the_queue_with_its_artists() {
    let tracks = parse_next(NEXT);
    let first = &tracks[0];
    assert_eq!(first.video_id, "kJQP7kiw5Fk");
    assert_eq!(first.title, "Despacito");
    assert_eq!(first.duration_secs, Some(282));
    assert_eq!(first.artists[0].name, "Luis Fonsi");
    assert!(first.artists.iter().any(|a| a.name == "Daddy Yankee"));
}

#[test]
fn up_next_tracks_have_durations() {
    let tracks = parse_next(NEXT);
    let with_duration = tracks.iter().filter(|t| t.duration_secs.is_some()).count();
    assert!(with_duration >= 5, "only {with_duration} tracks have a duration");
}

#[test]
fn up_next_does_not_repeat_a_video() {
    let tracks = parse_next(NEXT);
    let mut ids: Vec<&str> = tracks.iter().map(|t| t.video_id.as_str()).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "duplicate video ids in the queue");
}
