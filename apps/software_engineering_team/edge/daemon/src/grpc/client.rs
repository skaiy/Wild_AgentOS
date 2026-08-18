use tokio_stream::StreamExt;
use tonic::transport::Endpoint;

tonic::include_proto!("seapp");

pub struct KernelClient {
    client: se_kernel_service_client::SeKernelServiceClient<tonic::transport::Channel>,
}

impl KernelClient {
    pub async fn connect(target: &str) -> Result<Self, String> {
        let endpoint = Endpoint::new(target.to_string())
            .map_err(|e| format!("invalid kernel endpoint: {}", e))?;

        let channel = endpoint
            .connect_timeout(std::time::Duration::from_secs(5))
            .connect()
            .await
            .map_err(|e| format!("connect to kernel {} failed: {}", target, e))?;

        Ok(Self {
            client: se_kernel_service_client::SeKernelServiceClient::new(channel),
        })
    }

    pub async fn chat_stream(
        &self,
        prompt: String,
        task_iri: String,
        api_key: String,
        base_url: String,
        model: String,
    ) -> Result<String, String> {
        let req = ChatStreamRequest {
            prompt,
            task_iri,
            llm_api_key: api_key,
            llm_base_url: base_url,
            llm_model: model,
        };

        let mut client = self.client.clone();

        let mut stream = client
            .chat_stream(req)
            .await
            .map_err(|e| format!("ChatStream call failed: {}", e))?
            .into_inner();

        let mut full_content = String::new();

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    full_content.push_str(&chunk.content);
                    if chunk.done {
                        break;
                    }
                }
                Err(status) => {
                    return Err(format!("ChatStream stream error: {}", status));
                }
            }
        }

        if full_content.is_empty() {
            full_content = "Agent 已处理您的请求，但未返回具体内容。".to_string();
        }

        Ok(full_content)
    }
}