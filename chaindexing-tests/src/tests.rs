mod ingester;
mod integration;
mod repos;
mod states;

pub async fn setup() {
    states::setup().await;
}
