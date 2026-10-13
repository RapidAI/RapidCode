//! 模型供应商族 (Model Provider Family)
//!
//! Rust 移植自 `packages/shared/src/model-provider-family.ts`
//! （含 `model-provider-types.ts` / `oauth.ts` 的相关常量）。
//!
//! 负责把「内置模型供应商 ID」「OAuth 供应商」「base URL 域名」
//! 归一到 `zai` / `bigmodel` 两个「族」，并据此决定 UI 是否展示某族。

use std::collections::HashMap;

// ============================================================================
// 相关常量（来自 oauth.ts / model-provider-types.ts / zcodeEndpoint.ts）
// ============================================================================

/// OAuth 供应商 ID。
pub const BIGMODEL_PROVIDER_ID: &str = "bigmodel";
pub const ZAI_PROVIDER_ID: &str = "zai";

/// 内置模型供应商 ID。
pub mod builtin_model_provider_ids {
    pub const ZAI_INDIVIDUAL_CODING_PLAN: &str = "account:zai-individual-coding-plan";
    pub const ZAI_TEAM_CODING_PLAN: &str = "account:zai-team-coding-plan";
    pub const ZAI_START_PLAN: &str = "account:zai-start-plan";
    pub const BIGMODEL_INDIVIDUAL_CODING_PLAN: &str = "account:bigmodel-individual-coding-plan";
    pub const BIGMODEL_TEAM_CODING_PLAN: &str = "account:bigmodel-team-coding-plan";
    pub const BIGMODEL_START_PLAN: &str = "account:bigmodel-start-plan";
}

/// 默认 BigModel API 源（生产 / 测试一致）。
pub const DEFAULT_BIGMODEL_API_ORIGIN: &str = "https://bigmodel.cn";

// ============================================================================
// 族定义
// ============================================================================

/// 模型供应商族 ID。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelProviderFamilyId {
    Zai,
    BigModel,
}

impl ModelProviderFamilyId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelProviderFamilyId::Zai => "zai",
            ModelProviderFamilyId::BigModel => "bigmodel",
        }
    }
}

/// 族与域同义。
pub type ProviderFamilyDomain = ModelProviderFamilyId;

/// 一个供应商族的规格。
#[derive(Clone, Debug)]
pub struct ModelProviderFamilySpec {
    pub id: ModelProviderFamilyId,
    pub label: &'static str,
    pub root_domain: &'static str,
    pub oauth_provider_id: &'static str,
    pub start_plan_provider_id: &'static str,
    pub individual_coding_plan_provider_id: &'static str,
    pub team_coding_plan_provider_id: &'static str,
    pub team_coding_plan_manage_url: String,
}

/// 构造 BigModel 团队套餐管理 URL。
pub fn build_bigmodel_coding_plan_team_manage_url(api_origin: &str) -> String {
    let origin = api_origin.trim_end_matches('/');
    format!("{origin}/coding-plan/team/plans")
}

/// 所有供应商族规格。
pub fn model_provider_family_specs() -> Vec<ModelProviderFamilySpec> {
    use builtin_model_provider_ids::*;
    vec![
        ModelProviderFamilySpec {
            id: ModelProviderFamilyId::Zai,
            label: "Z.ai",
            root_domain: "z.ai",
            oauth_provider_id: ZAI_PROVIDER_ID,
            start_plan_provider_id: ZAI_START_PLAN,
            individual_coding_plan_provider_id: ZAI_INDIVIDUAL_CODING_PLAN,
            team_coding_plan_provider_id: ZAI_TEAM_CODING_PLAN,
            team_coding_plan_manage_url: "https://z.ai/manage-apikey/subscription".to_string(),
        },
        ModelProviderFamilySpec {
            id: ModelProviderFamilyId::BigModel,
            label: "BigModel",
            root_domain: "bigmodel.cn",
            oauth_provider_id: BIGMODEL_PROVIDER_ID,
            start_plan_provider_id: BIGMODEL_START_PLAN,
            individual_coding_plan_provider_id: BIGMODEL_INDIVIDUAL_CODING_PLAN,
            team_coding_plan_provider_id: BIGMODEL_TEAM_CODING_PLAN,
            team_coding_plan_manage_url: build_bigmodel_coding_plan_team_manage_url(
                DEFAULT_BIGMODEL_API_ORIGIN,
            ),
        },
    ]
}

/// 按族 ID 取规格。
pub fn get_model_provider_family_spec(
    family_id: ModelProviderFamilyId,
) -> Option<ModelProviderFamilySpec> {
    model_provider_family_specs().into_iter().find(|s| s.id == family_id)
}

/// providerId → 族 ID。
pub fn resolve_model_provider_family_id_by_provider_id(
    provider_id: &str,
) -> Option<ModelProviderFamilyId> {
    use builtin_model_provider_ids::*;
    let map: HashMap<&str, ModelProviderFamilyId> = [
        (ZAI_START_PLAN, ModelProviderFamilyId::Zai),
        (ZAI_INDIVIDUAL_CODING_PLAN, ModelProviderFamilyId::Zai),
        (ZAI_TEAM_CODING_PLAN, ModelProviderFamilyId::Zai),
        (BIGMODEL_START_PLAN, ModelProviderFamilyId::BigModel),
        (BIGMODEL_INDIVIDUAL_CODING_PLAN, ModelProviderFamilyId::BigModel),
        (BIGMODEL_TEAM_CODING_PLAN, ModelProviderFamilyId::BigModel),
    ]
    .into_iter()
    .collect();
    map.get(provider_id).copied()
}

/// 从 URL 中提取小写主机名（失败返回 None）。
pub fn extract_hostname(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let after_scheme = match trimmed.find("://") {
        Some(idx) => &trimmed[idx + 3..],
        None => return None,
    };
    // 去掉 path / query / fragment
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    // 去掉 userinfo 与端口
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

/// baseURL → 族 ID（按域名匹配）。
pub fn resolve_model_provider_family_id_by_base_url(
    base_url: Option<&str>,
) -> Option<ModelProviderFamilyId> {
    let trimmed = base_url.map(str::trim).filter(|s| !s.is_empty())?;
    let hostname = extract_hostname(trimmed)?;
    for spec in model_provider_family_specs() {
        if hostname == spec.root_domain || hostname.ends_with(&format!(".{}", spec.root_domain)) {
            return Some(spec.id);
        }
    }
    None
}

/// providerId → 族规格。
pub fn resolve_model_provider_family_spec_by_provider_id(
    provider_id: &str,
) -> Option<ModelProviderFamilySpec> {
    resolve_model_provider_family_id_by_provider_id(provider_id)
        .and_then(get_model_provider_family_spec)
}

/// providerId → 族标签。
pub fn resolve_model_provider_family_label_by_provider_id(provider_id: &str) -> Option<String> {
    resolve_model_provider_family_spec_by_provider_id(provider_id).map(|s| s.label.to_string())
}

/// 归一化族域字符串。
pub fn normalize_provider_family_domain(
    value: Option<&str>,
) -> Option<ProviderFamilyDomain> {
    match value {
        Some("zai") => Some(ModelProviderFamilyId::Zai),
        Some("bigmodel") => Some(ModelProviderFamilyId::BigModel),
        _ => None,
    }
}

/// OAuth 供应商 → 族域。
pub fn resolve_provider_family_domain_from_oauth_provider(
    provider: Option<&str>,
) -> Option<ProviderFamilyDomain> {
    match provider {
        Some(ZAI_PROVIDER_ID) => Some(ModelProviderFamilyId::Zai),
        Some(BIGMODEL_PROVIDER_ID) => Some(ModelProviderFamilyId::BigModel),
        _ => None,
    }
}

/// 是否展示某族。
pub fn should_show_model_provider_family_for_domain(
    family_id: ModelProviderFamilyId,
    provider_family_domain: Option<&str>,
) -> bool {
    match normalize_provider_family_domain(provider_family_domain) {
        None => true,
        Some(domain) => family_id == domain,
    }
}

/// 是否展示某族（依据活跃 OAuth 供应商）。
pub fn should_show_model_provider_family_for_active_oauth(
    family_id: ModelProviderFamilyId,
    active_oauth_provider: Option<&str>,
) -> bool {
    should_show_model_provider_family_for_domain(
        family_id,
        resolve_provider_family_domain_from_oauth_provider(active_oauth_provider)
            .map(|d| d.as_str()),
    )
}

/// 是否展示某内置供应商。
pub fn should_show_builtin_model_provider_for_domain(
    provider_id: &str,
    provider_family_domain: Option<&str>,
) -> bool {
    match resolve_model_provider_family_id_by_provider_id(provider_id) {
        None => true,
        Some(family_id) => {
            should_show_model_provider_family_for_domain(family_id, provider_family_domain)
        }
    }
}

/// 是否展示某内置供应商（依据活跃 OAuth 供应商）。
pub fn should_show_builtin_model_provider_for_active_oauth(
    provider_id: &str,
    active_oauth_provider: Option<&str>,
) -> bool {
    should_show_builtin_model_provider_for_domain(
        provider_id,
        resolve_provider_family_domain_from_oauth_provider(active_oauth_provider).map(|d| d.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_family_by_provider_id() {
        assert_eq!(
            resolve_model_provider_family_id_by_provider_id("account:zai-team-coding-plan"),
            Some(ModelProviderFamilyId::Zai)
        );
        assert_eq!(
            resolve_model_provider_family_label_by_provider_id("account:bigmodel-start-plan"),
            Some("BigModel".to_string())
        );
        assert_eq!(
            resolve_model_provider_family_id_by_provider_id("unknown"),
            None
        );
    }

    #[test]
    fn resolves_family_by_base_url() {
        assert_eq!(
            resolve_model_provider_family_id_by_base_url(Some("https://api.z.ai/v1")),
            Some(ModelProviderFamilyId::Zai)
        );
        assert_eq!(
            resolve_model_provider_family_id_by_base_url(Some("https://open.bigmodel.cn/api")),
            Some(ModelProviderFamilyId::BigModel)
        );
        assert_eq!(resolve_model_provider_family_id_by_base_url(Some("not a url")), None);
        assert_eq!(resolve_model_provider_family_id_by_base_url(None), None);
    }

    #[test]
    fn visibility_rules() {
        assert!(should_show_model_provider_family_for_domain(
            ModelProviderFamilyId::Zai,
            None
        ));
        assert!(should_show_model_provider_family_for_domain(
            ModelProviderFamilyId::Zai,
            Some("zai")
        ));
        assert!(!should_show_model_provider_family_for_domain(
            ModelProviderFamilyId::Zai,
            Some("bigmodel")
        ));
        assert!(should_show_builtin_model_provider_for_domain(
            "account:zai-start-plan",
            Some("zai")
        ));
        assert!(!should_show_builtin_model_provider_for_domain(
            "account:zai-start-plan",
            Some("bigmodel")
        ));
        // 非内置 provider 始终展示
        assert!(should_show_builtin_model_provider_for_domain("custom", Some("bigmodel")));
    }
}
