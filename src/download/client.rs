use crate::{input::parse_ids, model::Component};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone)]
pub(super) struct Client {
    http: reqwest::Client,
}
impl Client {
    /// 创建带请求标识、连接超时及总请求超时的 HTTP 客户端。 / Create an HTTP client with a user agent, connection timeout, and request timeout.
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 LcscDownloader/0.2")
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(45))
                .build()?,
        })
    }

    /// 获取并检查接口 JSON，最多三次尝试网络错误、限流或服务端错误。 / Fetch and check API JSON, allowing up to three attempts for transport, rate-limit, or server errors.
    async fn json(&self, url: &str) -> Result<Value> {
        let mut error = None;
        for attempt in 0..3 {
            match self.http.get(url).send().await {
                Ok(response) => {
                    if response.status() == 429 || response.status().is_server_error() {
                        error = Some(anyhow::anyhow!("立创接口暂时不可用：{}", response.status()));
                    } else {
                        let value: Value = response.error_for_status()?.json().await?;
                        ensure!(
                            value["success"] != false && value["code"].as_i64().unwrap_or(0) == 0,
                            "立创接口返回失败：{}",
                            value["message"]
                        );
                        return Ok(value);
                    }
                }
                Err(e) => error = Some(e.into()),
            }
            if attempt < 2 {
                tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
            }
        }
        Err(error.unwrap_or_else(|| anyhow::anyhow!("立创接口查询失败")))
    }

    /// 校验 C 编号，查询产品接口并返回精确匹配的规范化元数据。 / Validate a C ID, query the product API, and return normalized metadata for its exact match.
    pub async fn product(&self, id: &str) -> Result<Value> {
        ensure!(parse_ids(id) == [id], "无效的立创编号");
        let result = self
            .json(&format!(
                "https://pro.lceda.cn/api/szlcsc/eda/product/list?wd={id}"
            ))
            .await?;
        parse_product(&result, id)
    }

    /// 校验资源标识，并行获取符号与封装后转换为器件模型。 / Validate resource IDs, fetch the symbol and footprint concurrently, and convert them into a component.
    pub async fn component(&self, product: &Value) -> Result<Component> {
        let symbol = product["symbol"]["uuid"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("该器件没有符号资源")?;
        let footprint = product["footprint"]["uuid"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("该器件没有封装资源")?;
        ensure!(
            [symbol, footprint]
                .iter()
                .all(|uuid| uuid.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')),
            "资源 UUID 无效"
        );
        let symbol_url = format!("https://pro.lceda.cn/api/components/{symbol}?uuid={symbol}");
        let footprint_url =
            format!("https://pro.lceda.cn/api/components/{footprint}?uuid={footprint}");
        // 两项独立资源并行获取，全部成功后才允许转换。 / Fetch the independent resources concurrently and allow conversion only after both succeed.
        let (symbol, footprint) =
            tokio::try_join!(self.json(&symbol_url), self.json(&footprint_url))?;
        crate::source::convert(product, &symbol, &footprint)
    }
}

/// 按完整编号选择产品，并提取型号、描述及资源标识。 / Select a product by its complete ID and extract its model, description, and resource IDs.
pub(super) fn parse_product(payload: &Value, id: &str) -> Result<Value> {
    let items = payload["result"].as_array().context("没有搜索结果列表")?;
    // 按完整编号筛选搜索结果，避免导出相似型号。 / Filter search results by the complete ID to avoid exporting a similar component.
    let item = items
        .iter()
        .find(|item| {
            item["product_code"]
                .as_str()
                .or_else(|| item["attributes"]["Supplier Part"].as_str())
                .is_some_and(|code| code.eq_ignore_ascii_case(id))
        })
        .with_context(|| format!("未找到编号完全匹配的 {id}"))?;
    let model = item["attributes"]["Manufacturer Part"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| item["display_title"].as_str())
        .context("该器件没有型号")?;
    if model.trim().is_empty() {
        bail!("该器件型号为空");
    }
    let uuid = |kind: &str, key: &str| {
        item[kind]["uuid"]
            .as_str()
            .or_else(|| item["attributes"][key].as_str())
            .unwrap_or_default()
            .to_owned()
    };
    Ok(
        json!({"id":id,"model":model,"description":item["description"],"attributes":item["attributes"],"symbol":{"uuid":uuid("symbol","Symbol")},"footprint":{"uuid":uuid("footprint","Footprint")}}),
    )
}
