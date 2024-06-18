create extension if not exists vector cascade;

create table singers (
    id smallserial primary key,
    s_name varchar not null
);

create table songs (
    id bigserial not null primary key,
    title varchar not null,
    singer_id smallint references singers(id),
    -- TODO: make this utc or something idk
    date_first_sung date,
    -- TODO: not sure if this is the best way to store this, feels a bit out-of-scope
    local_path varchar
);

create table segments (
    song_id bigint not null references songs(id),
    segment_index bigint not null,
    -- vector is size of fft output as each is a line of the spectrogram
    vec vector(640) not null,
    start_ts_ms bigint not null,
    end_ts_ms bigint not null,
    
    duration_ms bigint generated always as (end_ts_ms - start_ts_ms) stored,

    primary key (song_id, segment_index)
);

-- for building index, consider using
-- SET max_parallel_maintenance_workers = 7;
-- SET maintenance_work_mem = '10GB';
create index on segments using hnsw (vec vector_l2_ops);

-- add known singers

insert into singers(id, s_name) values (0, 'neuro v1');
insert into singers(id, s_name) values (1, 'neuro v2');
insert into singers(id, s_name) values (2, 'neuro v3');
insert into singers(id, s_name) values (3, 'evil');
insert into singers(id, s_name) values (4, 'duet');
