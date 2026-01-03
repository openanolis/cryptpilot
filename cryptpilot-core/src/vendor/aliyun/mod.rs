use std::time::Duration;

use tokio::net::TcpStream;

pub mod cloudinit;
pub mod ntp;

pub async fn check_is_aliyun_ecs() -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(5),
            TcpStream::connect(("100.100.100.200", 80)),
        )
        .await,
        Ok(Ok(_))
    )
}
