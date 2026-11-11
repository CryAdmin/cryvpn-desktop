//! Проверка связи с кабинетом тем же клиентом, что стоит в приложении.
//! Печатает ошибку целиком, со всей цепочкой причин.
use std::error::Error;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let http = reqwest::Client::builder()
        .user_agent("CryVPN-Windows/probe")
        .cookie_store(true)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("client");

    for попытка in 1..=3 {
        let начало = std::time::Instant::now();
        match http
            .post("https://api.cryvps.com/api/v2/auth/refresh")
            .send()
            .await
        {
            Ok(о) => println!("popytka {попытка}: kod {} za {:?}", о.status(), начало.elapsed()),
            Err(е) => {
                println!("popytka {попытка}: OSHIBKA za {:?}: {е}", начало.elapsed());
                let mut причина = е.source();
                while let Some(п) = причина {
                    println!("    prichina: {п}");
                    причина = п.source();
                }
            }
        }
    }
}
