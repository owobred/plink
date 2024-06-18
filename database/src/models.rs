struct Singer {
    id: u16,
    name: String,
}

struct Song {
    id: u64,
    title: String,
    singer_id: u16,
}

struct Sample {
    song_id: u64,
    sample_index: u32,
    sample: Vec<f32>,
}