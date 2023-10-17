mod contract_states;
mod events_ingester;

pub async fn setup() {
    contract_states::setup().await;
}
