## Docker
1. Create a docker container running postgres with the [pgvector](https://github.com/pgvector/pgvector) extension installed
    1. Note this comes with a [Docker image](https://hub.docker.com/r/pgvector/pgvector)
    2. Also note that building the index for the database will use a *lot* of ram, so you need to specifcy a lot of ram for the container using `--shm-size`. For example, `docker run --shm-size=16GB -d -p 5432:5432 pgvector/pgvector`
2. Run the `database/schema.sql` script to create the schema and tables
    1. However, it may be wise to put off index initalization until the tables have been populated