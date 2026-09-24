from dataclasses import dataclass
from datetime import datetime, timezone

@dataclass(frozen=True)
class Profile:
    id: str
    enabled: bool = True
    budget_limit: float | None = None
    budget_used: float = 0.0
    reserve_amount: float = 0.0
    max_concurrency: int = 1
    running_jobs: int = 0
    priority: int = 0
    cooldown_until: str | None = None
    last_error: str | None = None
    last_used_at: str | None = None

    def available(self) -> bool:
        if not self.enabled or self.running_jobs >= self.max_concurrency or self.last_error:
            return False
        if self.budget_limit is not None and self.budget_limit - self.budget_used <= self.reserve_amount:
            return False
        return not self.cooldown_until or self.cooldown_until <= datetime.now(timezone.utc).isoformat()

def choose_profile(profiles: list[Profile]) -> Profile | None:
    candidates = [p for p in profiles if p.available()]
    return sorted(candidates, key=lambda p: (-p.priority, p.running_jobs, (p.budget_used / p.budget_limit if p.budget_limit else 0), p.last_used_at or ""))[0] if candidates else None
