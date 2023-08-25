use diesel::pg::PgConnection;
use diesel::prelude::*;
use dotenvy::dotenv;
use std::env;

pub fn setup() {
    let db_url = database_url();

    let _conn = PgConnection::establish(&db_url).unwrap_or_else(|_error| {
        let (db_name, db_raw_url) = get_db_name_and_raw_url(&db_url);

        let mut raw_conn = connect_to_database_url_or_panic(&db_raw_url);

        create_database(&db_name, &mut raw_conn);

        connect()
    });
}

fn connect() -> PgConnection {
    connect_to_database_url_or_panic(&database_url())
}

pub fn database_url() -> String {
    dotenv().ok();

    env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL env variable needs to be set.")
}

fn get_db_name_and_raw_url(url: &str) -> (String, String) {
    let mut url_split = url.split('/').collect::<Vec<&str>>();

    let db_name = url_split
        .pop()
        .expect("DATABASE NAME needs to be specified. See: sample.env");
    let db_raw_url = url_split.join("/");

    (db_name.to_string(), db_raw_url)
}

#[allow(clippy::uninlined_format_args)]
fn create_database(db_name: &str, conn: &mut PgConnection) {
    diesel::sql_query(format!(r#"CREATE DATABASE "{}""#, db_name))
        .execute(conn)
        .unwrap();
}

#[allow(clippy::uninlined_format_args)]
fn connect_to_database_url_or_panic(db_url: &str) -> PgConnection {
    PgConnection::establish(db_url).unwrap_or_else(|_| panic!("Error connecting to {}", db_url))
}
