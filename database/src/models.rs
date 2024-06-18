#[derive(Debug)]
pub struct Singer {
    pub id: u16,
    pub name: String,
}

#[derive(Debug)]

pub struct Song {
    pub id: u64,
    pub metadata: SongMetadata,
}

#[derive(Debug)]
pub struct SongMetadata {
    pub title: String,
    pub singer_id: u16,
    pub date_first_sung: time::Date,
    pub local_path: String,
}

#[derive(Debug)]
pub struct Sample {
    pub song_id: u64,
    pub sample_index: u32,
    pub sample: Vec<f32>,
}
