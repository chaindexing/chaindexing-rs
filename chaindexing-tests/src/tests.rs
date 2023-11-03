mod contract_states;
mod events_ingester;
mod repos;

pub async fn setup() {
    contract_states::setup().await;
}
