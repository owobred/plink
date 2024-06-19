#[derive(Debug)]
pub struct Singer {
    pub id: i16,
    pub name: String,
}

#[derive(Debug)]

pub struct Song {
    pub id: i64,
    pub metadata: SongMetadata,
}

#[derive(Debug)]
pub struct SongMetadata {
    pub title: String,
    pub singer_id: i16,
    pub date_first_sung: Option<time::Date>,
    pub local_path: Option<String>,
}

#[derive(Debug)]
pub struct Sample {
    pub song_id: u64,
    pub sample_index: u32,
    pub sample: Vec<f32>,
}
