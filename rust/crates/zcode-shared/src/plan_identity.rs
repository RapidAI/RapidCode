//! 套餐身份解析 (Plan Identity)
//!
//! Rust 移植自 `packages/shared/src/plan-identity.ts`（含 `usage-stats.ts`
//! 相关结构的最小子集）。

use crate::model_provider_family::ProviderFamilyDomain;

/// 套餐身份状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanIdentityStatus {
    Unknown,
    CodingPlan,
    StartPlan,
    NoPlan,
}

impl PlanIdentityStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanIdentityStatus::Unknown => "unknown",
            PlanIdentityStatus::CodingPlan => "coding_plan",
            PlanIdentityStatus::StartPlan => "start_plan",
            PlanIdentityStatus::NoPlan => "no_plan",
        }
    }
}

/// 套餐身份快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanIdentitySnapshot {
    pub generated_at: i64,
    pub plan_status: PlanIdentityStatus,
    pub plan_product_id: String,
}

/// 权益不可用原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    Unavailable,
    NotAuthenticated,
    NotConfigured,
    NoPlan,
}

/// 用量权益快照（最小子集）。
#[derive(Clone, Debug, Default)]
pub struct UsageEntitlementSnapshot {
    pub generated_at: i64,
    pub authenticated: bool,
    pub unavailable_reason: Option<UnavailableReason>,
    pub has_quota: bool,
    pub has_remaining: bool,
    /// 订阅明细中的首个 productId（若有）。
    pub subscription_product_id: Option<String>,
}

impl UsageEntitlementSnapshot {
    fn has_subscription(&self) -> bool {
        self.subscription_product_id.is_some()
    }
}

/// 解析输入。
pub struct ResolvePlanIdentityInput<'a> {
    pub provider_family_domain: Option<ProviderFamilyDomain>,
    pub coding_plan_entitlement: Option<&'a UsageEntitlementSnapshot>,
    pub start_plan_entitlement: Option<&'a UsageEntitlementSnapshot>,
    pub now: i64,
    pub entitlement_cache_ttl_ms: i64,
}

enum EntitlementState {
    Active {
        generated_at: i64,
        plan_product_id: String,
    },
    None,
    Unknown,
}

/// 解析套餐身份快照。
pub fn resolve_plan_identity_snapshot(input: ResolvePlanIdentityInput<'_>) -> PlanIdentitySnapshot {
    let unknown = create_snapshot(input.now, PlanIdentityStatus::Unknown, "");
    let Some(_domain) = input.provider_family_domain else {
        return unknown;
    };

    let coding_state = resolve_plan_entitlement_state(
        input.coding_plan_entitlement,
        input.now,
        input.entitlement_cache_ttl_ms,
    );
    match coding_state {
        EntitlementState::Active {
            generated_at,
            plan_product_id,
        } => {
            return create_snapshot(
                generated_at,
                PlanIdentityStatus::CodingPlan,
                &plan_product_id,
            )
        }
        EntitlementState::Unknown => return unknown,
        EntitlementState::None => {}
    }

    let start_state = resolve_plan_entitlement_state(
        input.start_plan_entitlement,
        input.now,
        input.entitlement_cache_ttl_ms,
    );
    match start_state {
        EntitlementState::Active {
            generated_at,
            plan_product_id,
        } => create_snapshot(generated_at, PlanIdentityStatus::StartPlan, &plan_product_id),
        EntitlementState::None => create_snapshot(input.now, PlanIdentityStatus::NoPlan, ""),
        EntitlementState::Unknown => unknown,
    }
}

fn create_snapshot(
    generated_at: i64,
    plan_status: PlanIdentityStatus,
    plan_product_id: &str,
) -> PlanIdentitySnapshot {
    PlanIdentitySnapshot {
        generated_at,
        plan_status,
        plan_product_id: plan_product_id.to_string(),
    }
}

fn resolve_plan_entitlement_state(
    snapshot: Option<&UsageEntitlementSnapshot>,
    now: i64,
    entitlement_cache_ttl_ms: i64,
) -> EntitlementState {
    let Some(snapshot) = snapshot else {
        return EntitlementState::Unknown;
    };
    if now - snapshot.generated_at > entitlement_cache_ttl_ms {
        return EntitlementState::Unknown;
    }
    if !snapshot.authenticated
        || snapshot.unavailable_reason == Some(UnavailableReason::Unavailable)
    {
        return EntitlementState::Unknown;
    }
    if snapshot.unavailable_reason == Some(UnavailableReason::NotAuthenticated)
        || snapshot.unavailable_reason == Some(UnavailableReason::NotConfigured)
    {
        return EntitlementState::Unknown;
    }
    if snapshot.unavailable_reason == Some(UnavailableReason::NoPlan) {
        return EntitlementState::None;
    }
    if snapshot.has_quota || snapshot.has_subscription() || snapshot.has_remaining {
        return EntitlementState::Active {
            generated_at: snapshot.generated_at,
            plan_product_id: snapshot.subscription_product_id.clone().unwrap_or_default(),
        };
    }
    EntitlementState::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_snapshot(generated_at: i64, product: &str) -> UsageEntitlementSnapshot {
        UsageEntitlementSnapshot {
            generated_at,
            authenticated: true,
            unavailable_reason: None,
            has_quota: true,
            has_remaining: false,
            subscription_product_id: Some(product.to_string()),
        }
    }

    #[test]
    fn no_domain_is_unknown() {
        let snap = resolve_plan_identity_snapshot(ResolvePlanIdentityInput {
            provider_family_domain: None,
            coding_plan_entitlement: None,
            start_plan_entitlement: None,
            now: 1000,
            entitlement_cache_ttl_ms: 60_000,
        });
        assert_eq!(snap.plan_status, PlanIdentityStatus::Unknown);
    }

    #[test]
    fn coding_plan_wins() {
        let coding = active_snapshot(1000, "coding-pro");
        let snap = resolve_plan_identity_snapshot(ResolvePlanIdentityInput {
            provider_family_domain: Some(ProviderFamilyDomain::Zai),
            coding_plan_entitlement: Some(&coding),
            start_plan_entitlement: None,
            now: 1000,
            entitlement_cache_ttl_ms: 60_000,
        });
        assert_eq!(snap.plan_status, PlanIdentityStatus::CodingPlan);
        assert_eq!(snap.plan_product_id, "coding-pro");
    }

    #[test]
    fn expired_entitlement_is_unknown() {
        let coding = active_snapshot(1000, "coding-pro");
        let snap = resolve_plan_identity_snapshot(ResolvePlanIdentityInput {
            provider_family_domain: Some(ProviderFamilyDomain::Zai),
            coding_plan_entitlement: Some(&coding),
            start_plan_entitlement: None,
            now: 100_000,
            entitlement_cache_ttl_ms: 60_000,
        });
        assert_eq!(snap.plan_status, PlanIdentityStatus::Unknown);
    }

    #[test]
    fn no_plan_when_both_explicit() {
        // 只有当 coding 与 start 都明确为 no_plan 时，结果才是 NoPlan。
        let no_plan = UsageEntitlementSnapshot {
            generated_at: 1000,
            authenticated: true,
            unavailable_reason: Some(UnavailableReason::NoPlan),
            ..Default::default()
        };
        let snap = resolve_plan_identity_snapshot(ResolvePlanIdentityInput {
            provider_family_domain: Some(ProviderFamilyDomain::BigModel),
            coding_plan_entitlement: Some(&no_plan),
            start_plan_entitlement: Some(&no_plan),
            now: 1000,
            entitlement_cache_ttl_ms: 60_000,
        });
        assert_eq!(snap.plan_status, PlanIdentityStatus::NoPlan);
    }

    #[test]
    fn unknown_coding_entitlement_short_circuits() {
        // coding 权益未知（缺失）时直接返回 unknown，不再看 start（与 TS 一致）。
        let start = active_snapshot(1000, "start-pro");
        let snap = resolve_plan_identity_snapshot(ResolvePlanIdentityInput {
            provider_family_domain: Some(ProviderFamilyDomain::Zai),
            coding_plan_entitlement: None,
            start_plan_entitlement: Some(&start),
            now: 1000,
            entitlement_cache_ttl_ms: 60_000,
        });
        assert_eq!(snap.plan_status, PlanIdentityStatus::Unknown);
    }
}
