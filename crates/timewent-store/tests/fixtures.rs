//! Recorded fixture streams survive the store unchanged: same samples, same segments.

use std::fs;
use std::path::PathBuf;

use timewent_core::{segment, Config, Sample};
use timewent_store::Store;

fn fixtures() -> Vec<(String, Vec<Sample>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut out: Vec<(String, Vec<Sample>)> = fs::read_dir(&dir)
        .expect("fixtures dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .map(|p| {
            let text = fs::read_to_string(&p).expect("read fixture");
            let samples = text
                .lines()
                .map(|l| serde_json::from_str(l).expect("sample line"))
                .collect();
            (p.display().to_string(), samples)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn fixtures_round_trip_through_the_store_with_identical_segments() {
    let all = fixtures();
    assert!(!all.is_empty(), "no fixtures found");
    let config = Config::default();
    for (name, samples) in all {
        let mut store = Store::open_in_memory().expect("store");
        let first = samples.first().expect("non-empty fixture").ts_ms;
        let last = samples.last().expect("non-empty fixture").ts_ms;
        let id = store.start_session(first).expect("start");
        for s in &samples {
            store.append(id, s).expect("append");
        }
        store.end_session(id, last + 1_000).expect("end");

        let back = store.samples(id).expect("samples");
        assert_eq!(back, samples, "{name}: samples differ");
        assert_eq!(
            segment(&back, &config),
            segment(&samples, &config),
            "{name}: segments differ"
        );
        assert_eq!(
            store.samples_between(first, last + 1).expect("between"),
            samples,
            "{name}: range read differs"
        );
    }
}
