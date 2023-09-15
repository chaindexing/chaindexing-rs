pub mod db;
pub mod factory;
pub mod test_runner;
/// In actual sense, this only tests the postgres repo. A better name for this lib would be
/// chaindexing-postgres-tests but this will be a concern when we support other repos
mod tests;
