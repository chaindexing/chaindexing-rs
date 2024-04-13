mod events_ingester;
mod repos;
mod states;

pub async fn setup() {
    states::setup().await;
}
