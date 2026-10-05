"""Pier installed-agent adapter that runs Gold Band AUTO headlessly.

Usage:
    PYTHONPATH=/root/gb-bench/adapter pier run -p deep-swe/tasks \
        --agent-import-path gold_band_agent:GoldBandAuto -m deepseek-flash

Host environment (read via Pier's --ae / process env):
    ANTHROPIC_BASE_URL, ANTHROPIC_AUTH_TOKEN  model endpoint for claude-agent-acp
    GOLD_BAND_BINARY                          Linux gold-band binary on the host
    GOLD_BAND_EFFORT                          claude-acp `effort` option (default: max)
"""

import io
import json
import os
import tarfile
import tempfile
from pathlib import Path
from urllib.parse import urlparse

from pier.agents.installed.base import BaseInstalledAgent
from pier.environments.base import BaseEnvironment
from pier.models.agent.context import AgentContext
from pier.models.agent.install import AgentInstallSpec, InstallStep
from pier.models.agent.network import NetworkAllowlist

CLAUDE_ACP_PACKAGE = "@agentclientprotocol/claude-agent-acp@0.81.2"
AGENT_TYPE = "claude-acp"
INSTALL_DIR = "/installed-agent"
GOLD_BAND_BIN = f"{INSTALL_DIR}/gold-band"
GOLD_BAND_HOME = f"{INSTALL_DIR}/home"
REQUIREMENT_FILE = f"{INSTALL_DIR}/requirement.md"
AUTO_CONFIG_FILE = f"{INSTALL_DIR}/auto.json"
AGENT_LOG_DIR = "/logs/agent"
WORKDIR = "/app"

# Roles AUTO may assign; interactive roles (interview, grill) and cleanup/cicd are excluded.
ALLOWED_PROFILES = [
    "pf-builtin-plan",
    "pf-builtin-dev",
    "pf-builtin-dev-test",
    "pf-builtin-test",
    "pf-builtin-review",
    "pf-builtin-accept",
]
# Dynamic budget: nested fanout (group depth 2) and a larger node/depth budget.
DYNAMIC_CONTROL = {
    "maxDynamicNodes": 40,
    "maxFanout": 5,
    "maxDepth": 10,
    "maxParallel": 3,
    "maxGroupDepth": 2,
    "maxWorkflowInvocations": 10,
    "allowNestedDynamic": False,
}
# gold-band exit codes: 2 = paused, 3 = command failed, 4 = not settled.
# 3/4 never produced a gradable run. A pause (2) is graded as the agent's own
# result when its runtime error is Gold Band's dynamic protocol (for example
# completion repair exhausted); any other pause is an infrastructure outcome.
INFRA_EXIT_CODES = {"3", "4"}
PAUSED_EXIT_CODE = "2"
AGENT_RUNTIME_ERROR_PREFIX = "dynamic."
# Provider-side failures that invalidate a trial even if the run completed.
PROVIDER_FAILURE_PATTERN = "Insufficient Balance|insufficient_balance"


class GoldBandInfraError(RuntimeError):
    """Trial invalid for infrastructure reasons; rerun with
    `pier job resume -p <job> -f GoldBandInfraError`."""


class GoldBandAuto(BaseInstalledAgent):
    @staticmethod
    def name() -> str:
        return "gold-band-auto"

    def get_version_command(self) -> str | None:
        return None

    def install_spec(self) -> AgentInstallSpec:
        # Runs at image build time (network available). Warms the npx cache so
        # Gold Band's catalog launch `npx -y <pkg>` resolves offline at run time.
        warm = (
            "set -eu; "
            f"npm exec --yes --package={CLAUDE_ACP_PACKAGE} -- node -e 0; "
            f"mkdir -p {INSTALL_DIR}"
        )
        return AgentInstallSpec(
            agent_name=self.name(),
            version=self._version,
            steps=[InstallStep(user="root", run=warm)],
            verification_command=None,
        )

    def network_allowlist(self) -> NetworkAllowlist:
        base_url = self._get_env("ANTHROPIC_BASE_URL") or ""
        host = urlparse(base_url if "://" in base_url else f"https://{base_url}").hostname
        if not host:
            raise RuntimeError("ANTHROPIC_BASE_URL is required")
        return NetworkAllowlist(domains=[host])

    async def setup(self, environment: BaseEnvironment) -> None:
        await super().setup(environment)
        binary = os.environ.get("GOLD_BAND_BINARY")
        if not binary or not Path(binary).is_file():
            raise RuntimeError("GOLD_BAND_BINARY must point to the Linux gold-band binary")
        await environment.upload_file(binary, GOLD_BAND_BIN)
        await self.exec_as_root(environment, command=f"chmod +x {GOLD_BAND_BIN}")

    def _model(self) -> str:
        model = self.model_name or self._get_env("ANTHROPIC_MODEL")
        if not model:
            raise RuntimeError("model is required (-m)")
        return model

    def _auto_config(self) -> dict:
        # The model is selected through ANTHROPIC_MODEL; claude-acp keeps its
        # default model option, which resolves to that environment value.
        return {
            "agentType": AGENT_TYPE,
            "permissionMode": "bypassPermissions",
            "autoAccept": True,
            "configOptions": {"effort": os.environ.get("GOLD_BAND_EFFORT", "max")},
            "allowedProfiles": ALLOWED_PROFILES,
            "control": DYNAMIC_CONTROL,
        }

    def _agent_env(self) -> dict[str, str]:
        model = self._model()
        env = {
            "ANTHROPIC_BASE_URL": self._get_env("ANTHROPIC_BASE_URL") or "",
            "ANTHROPIC_AUTH_TOKEN": self._get_env("ANTHROPIC_AUTH_TOKEN") or "",
            "ANTHROPIC_MODEL": model,
            "ANTHROPIC_DEFAULT_OPUS_MODEL": model,
            "ANTHROPIC_DEFAULT_SONNET_MODEL": model,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": model,
            "CLAUDE_CODE_SUBAGENT_MODEL": model,
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
            "CLAUDE_CODE_ATTRIBUTION_HEADER": "0",
            "IS_SANDBOX": "1",
            "GOLD_BAND_HOME": GOLD_BAND_HOME,
            # npx must resolve claude-agent-acp from the warmed cache only.
            "npm_config_offline": "true",
        }
        env = self.build_process_env(env, include_resolved_env=False)
        return {k: v for k, v in env.items() if v}

    async def run(
        self, instruction: str, environment: BaseEnvironment, context: AgentContext
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            requirement = Path(tmp) / "requirement.md"
            requirement.write_text(instruction, encoding="utf-8")
            auto_config = Path(tmp) / "auto.json"
            auto_config.write_text(json.dumps(self._auto_config()), encoding="utf-8")
            await environment.upload_file(requirement, REQUIREMENT_FILE)
            await environment.upload_file(auto_config, AUTO_CONFIG_FILE)

        command = (
            f"cd {WORKDIR} && mkdir -p {GOLD_BAND_HOME} {AGENT_LOG_DIR} && "
            f"{GOLD_BAND_BIN} run --requirement-file {REQUIREMENT_FILE} "
            f"--auto-config {AUTO_CONFIG_FILE} "
            f"> {AGENT_LOG_DIR}/run-state.json 2> {AGENT_LOG_DIR}/gold-band.stderr; "
            f"echo $? > {AGENT_LOG_DIR}/exit-code"
        )
        try:
            await self.exec_as_agent(environment, command=command, env=self._agent_env())
        finally:
            await self.exec_as_agent(
                environment,
                command=(
                    f"cd {WORKDIR}; git status --short > {AGENT_LOG_DIR}/final-status.txt 2>&1; "
                    f"git log --oneline -20 --all > {AGENT_LOG_DIR}/final-log.txt 2>&1; "
                    f"tar czf {AGENT_LOG_DIR}/gold-band-home.tgz -C {INSTALL_DIR} home 2>/dev/null; "
                    f"grep -rhoE '{PROVIDER_FAILURE_PATTERN}' {GOLD_BAND_HOME} 2>/dev/null "
                    f"| sort | uniq -c > {AGENT_LOG_DIR}/provider-failures.txt; true"
                ),
            )
        self._raise_if_infra_failure()

    def _runtime_error_codes(self) -> set[str]:
        """Codes of `dynamic_runtime_error` events in the archived Gold Band home."""
        archive = self.logs_dir / "gold-band-home.tgz"
        codes: set[str] = set()
        if not archive.is_file():
            return codes
        with tarfile.open(archive) as tar:
            for member in tar:
                if not member.name.endswith("/dynamic/events.jsonl"):
                    continue
                for line in io.TextIOWrapper(tar.extractfile(member), encoding="utf-8"):
                    event = json.loads(line)
                    if event.get("type") != "dynamic_runtime_error":
                        continue
                    code = ((event.get("data") or {}).get("runtimeError") or {}).get("code") or {}
                    codes.add(code.get("code") or "unknown")
        return codes

    def _paused_by_agent(self) -> bool:
        codes = self._runtime_error_codes()
        return bool(codes) and all(code.startswith(AGENT_RUNTIME_ERROR_PREFIX) for code in codes)

    def _raise_if_infra_failure(self) -> None:
        exit_file = self.logs_dir / "exit-code"
        exit_code = exit_file.read_text().strip() if exit_file.is_file() else "missing"
        if exit_code == PAUSED_EXIT_CODE and self._paused_by_agent():
            return
        if exit_code == "missing" or exit_code in INFRA_EXIT_CODES or exit_code == PAUSED_EXIT_CODE:
            pause = ""
            state = self.logs_dir / "run-state.json"
            if state.is_file():
                try:
                    run = json.loads(state.read_text())
                    pause = json.dumps(run.get("pause_reason") or run.get("pauseReason"))[:500]
                except ValueError:
                    pause = "unparseable run-state"
            raise GoldBandInfraError(f"gold-band exit={exit_code} pause={pause}")
        failures = self.logs_dir / "provider-failures.txt"
        if failures.is_file() and failures.read_text().strip():
            raise GoldBandInfraError(f"provider failures: {failures.read_text().strip()[:500]}")

    def populate_context_post_run(self, context: AgentContext) -> None:
        exit_code = self.logs_dir / "exit-code"
        if exit_code.is_file():
            context.metadata = {
                **(context.metadata or {}),
                "gold_band_exit_code": exit_code.read_text().strip(),
            }


__all__ = ["GoldBandAuto", "GoldBandInfraError"]
