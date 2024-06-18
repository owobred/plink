use pgvector::Vector;
use tracing::{debug, instrument};

pub mod models;

pub struct Database {
    pool: sqlx::Pool<sqlx::Postgres>,
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = sqlx::PgPool::connect(url).await?;

        Ok(Self { pool })
    }

    pub async fn find_similar_to(
        &self,
        vector: Vector,
        thresh: f32,
    ) -> Result<Vec<Vector>, sqlx::Error> {
        let result: Vec<(f32,)> = sqlx::query_as(
            "
            select vec <-> $1 from segments
            where vec <-> $1 < $2
            order by vec <-> $1
        ",
        )
        .bind(vector)
        .bind(thresh)
        .fetch_all(&self.pool)
        .await?;

        println!("{result:?}");

        todo!()
    }

    #[instrument(skip(self, spectrogram), ret, level = "trace")]
    pub async fn insert_new_song(
        &self,
        spectrogram: Vec<Vec<f32>>,
        metadata: &models::SongMetadata,
        samplerate: usize,
        fft_size: usize,
        fft_overlap: usize,
    ) -> Result<i64, sqlx::Error> {
        let (song_id,): (i64,) = sqlx::query_as(
            "
            insert into songs(title, singer_id, date_first_sung, local_path)
            values ($1, $2, $3, $4)
            returning id
        ",
        )
        .bind(&metadata.title)
        .bind(metadata.singer_id as i16)
        .bind(metadata.date_first_sung)
        .bind(&metadata.local_path)
        .fetch_one(&self.pool)
        .await?;

        self.insert_sectrogram_for_song(song_id, spectrogram, samplerate, fft_size, fft_overlap).await?;

        Ok(song_id)
    }

    #[instrument(skip(self, spectrogram), level = "trace")]
    async fn insert_sectrogram_for_song(
        &self,
        song_id: i64,
        spectrogram: Vec<Vec<f32>>,
        samplerate: usize,
        fft_size: usize,
        fft_overlap: usize,
    ) -> Result<(), sqlx::Error> {
        let (segments,): (i64,) =
            sqlx::query_as("select count(*) from segments where song_id = $1")
                .bind(song_id)
                .fetch_one(&self.pool)
                .await?;

        if segments > 0 {
            panic!("song already exists, not inserting new values");
        }

        let fft_offset = fft_size - fft_overlap;

        let mut connection = self.pool.acquire().await?;
        let mut copy_in = connection.copy_in_raw("copy segments(song_id, segment_index, vec, start_ts_ms, end_ts_ms) from stdin with (format csv, delimiter '|', header false)").await?;

        for (index, segment) in spectrogram.into_iter().enumerate() {
            let start_offset = fft_offset * index;
            let start_time_ms = (start_offset as f64 * 1.0 / samplerate as f64 * 1000.0) as i64;
            let end_offset = fft_offset * index + fft_size;
            let end_time_ms = (end_offset as f64 * 1.0 / samplerate as f64 * 1000.0) as i64;
            copy_in
                .send(
                    format!(
                        "{song_id}|{index}|{}|{start_time_ms}|{end_time_ms}\n",
                        format!("{segment:?}").replace(" ", "")
                    )
                    .as_bytes(),
                )
                .await?;
        }
        let rows_affected = copy_in.finish().await?;
        debug!(n_rows = rows_affected, "affected rows");

        Ok(())
    }
}
