/*!
 * IAM 身份提取中间件（G7）。
 *
 * 优先级：
 *   1. Authorization: Bearer <HS256 JWT>  —— 生产级签名验证
 *   2. X-Identity: base64(JSON)            —— 开发/测试模拟身份
 *   3. 匿名（user_id="anonymous"）         —— 无凭据回退
 *
 * AGENTOS_JWT_SECRET 环境变量控制签名密钥。
 * AGENTOS_AUTH_STRICT=true 强制执行角色校验（默认 false，适合本地开发）。
 */

use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ─── JWT Claims ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject = user_id
    pub sub: String,
    pub tenant_id: String,
    #[serde(default)]
    pub roles: Vec<String>,
    /// Unix timestamp 过期时间。
    pub exp: usize,
}

// ─── UserIdentity ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AuthMethod { Jwt, Base64Header, Anonymous }

#[derive(Debug, Clone)]
pub struct UserIdentity {
    pub user_id: String,
    pub tenant_id: String,
    pub roles: Vec<String>,
    pub auth_method: AuthMethod,
}

impl UserIdentity {
    pub fn anonymous() -> Self {
        Self { user_id: "anonymous".to_string(), tenant_id: "default".to_string(), roles: vec![], auth_method: AuthMethod::Anonymous }
    }
    /// 检查调用方是否具有指定角色（任一匹配）。
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r.as_str() == role)
    }
    /// 在严格模式下检查角色；非严格模式下匿名用户放行（便于本地开发）。
    pub fn require_role(&self, role: &str) -> Result<(), (StatusCode, Json<Value>)> {
        if self.has_role(role) { return Ok(()); }
        let strict = std::env::var("AGENTOS_AUTH_STRICT").as_deref() == Ok("true");
        if !strict && self.auth_method == AuthMethod::Anonymous { return Ok(()); }
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "forbidden",
                "required_role": role,
                "user_id": self.user_id,
                "user_roles": self.roles,
                "hint": "Set AGENTOS_AUTH_STRICT=false to bypass role checks in dev mode",
            })),
        ))
    }
}

// ─── Axum Extractor ───────────────────────────────────────────────────────────

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for UserIdentity {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // 1. JWT Bearer
        if let Some(auth) = parts.headers.get("authorization") {
            if let Ok(val) = auth.to_str() {
                if let Some(token) = val.strip_prefix("Bearer ") {
                    if let Some(identity) = verify_jwt(token) {
                        return Ok(identity);
                    }
                }
            }
        }
        // 2. X-Identity base64-JSON（开发模拟）
        if let Some(hdr) = parts.headers.get("x-identity") {
            if let Ok(val) = hdr.to_str() {
                if let Ok(bytes) = STANDARD.decode(val) {
                    if let Ok(claims) = serde_json::from_slice::<Value>(&bytes) {
                        return Ok(UserIdentity {
                            user_id: str_field(&claims, "user_id", "anonymous"),
                            tenant_id: str_field(&claims, "tenant_id", "default"),
                            roles: arr_field(&claims, "roles"),
                            auth_method: AuthMethod::Base64Header,
                        });
                    }
                }
            }
        }
        // 3. Anonymous fallback
        Ok(UserIdentity::anonymous())
    }
}

// ─── JWT 验签 ─────────────────────────────────────────────────────────────────

fn jwt_secret() -> String {
    std::env::var("AGENTOS_JWT_SECRET")
        .unwrap_or_else(|_| "agentos-dev-secret-change-in-prod".to_string())
}

fn verify_jwt(token: &str) -> Option<UserIdentity> {
    use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
    let key = DecodingKey::from_secret(jwt_secret().as_bytes());
    let mut val = Validation::new(Algorithm::HS256);
    val.set_required_spec_claims(&["sub", "exp"]);
    match decode::<JwtClaims>(token, &key, &val) {
        Ok(data) => Some(UserIdentity {
            user_id: data.claims.sub,
            tenant_id: data.claims.tenant_id,
            roles: data.claims.roles,
            auth_method: AuthMethod::Jwt,
        }),
        Err(e) => { tracing::debug!("JWT verify failed: {}", e); None }
    }
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn str_field(v: &Value, key: &str, default: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or(default).to_string()
}
fn arr_field(v: &Value, key: &str) -> Vec<String> {
    v.get(key).and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|r| r.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}
