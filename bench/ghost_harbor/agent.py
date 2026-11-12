"""Run ghost-work as an agent under Harbor (Terminal-Bench 2.x and later).

The loop runs on the host, where it is built. Every command it issues goes
into the task's container through `docker exec`. That is the same shape a
ghost has on a server: her mind outside, her shell inside a sandbox.

    export DEEPSEEK_API_KEY=...
    export GHOST_WORK_BIN=$PWD/target/release/ghost-work
    cd bench && harbor run -p <tasks> -a ghost_harbor.agent:GhostAgent \
        -m deepseek/deepseek-flash

Works with Harbor's local Docker environment only: it finds the container by
the Compose project Harbor names after the trial.
"""

import asyncio
import json
import os
import re
from pathlib import Path

from harbor.agents.base import BaseAgent
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

# USD per million tokens, DeepSeek flash off-peak. Used only to fill in the
# cost column; the token counts are what the comparison rests on.
PRICE = {"cached": 0.003, "fresh": 0.15, "output": 0.60}


def _project(session_id: str) -> str:
    """The Compose project name Harbor derives from a trial's session id."""
    name = session_id.lower()
    if not re.match(r"^[a-z0-9]", name):
        name = "0" + name
    return re.sub(r"[^a-z0-9_-]", "-", name)


class GhostAgent(BaseAgent):
    def __init__(
        self,
        *args,
        binary: str | None = None,
        max_steps: int = 200,
        max_minutes: int = 60,
        context_tokens: int = 60000,
        base_url: str = "https://api.deepseek.com",
        api_key_env: str = "DEEPSEEK_API_KEY",
        **kwargs,
    ):
        super().__init__(*args, **kwargs)
        self._bin = Path(binary or os.environ["GHOST_WORK_BIN"]).expanduser()
        self._max_steps = int(max_steps)
        self._max_minutes = int(max_minutes)
        self._context_tokens = int(context_tokens)
        self._base_url = base_url
        self._api_key_env = api_key_env

    @staticmethod
    def name() -> str:
        return "ghost-work"

    def version(self) -> str | None:
        return "0.0.1"

    async def setup(self, environment: BaseEnvironment) -> None:
        # Nothing is installed in the container: the loop is not in it.
        return None

    async def _container(self, environment: BaseEnvironment) -> str:
        project = _project(environment.session_id)
        proc = await asyncio.create_subprocess_exec(
            "docker", "ps", "-q",
            "--filter", f"label=com.docker.compose.project={project}",
            "--filter", "label=com.docker.compose.service=main",
            stdout=asyncio.subprocess.PIPE,
        )
        out, _ = await proc.communicate()
        ids = out.decode().split()
        if len(ids) != 1:
            raise RuntimeError(f"expected one main container for {project}, found {ids}")
        return ids[0]

    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        container = await self._container(environment)
        state = self.logs_dir / "state"
        model = (self.model_name or "deepseek/deepseek-flash").split("/", 1)[-1]
        command = [
            str(self._bin),
            "--task", instruction,
            "--state", str(state),
            "--docker", container,
            "--model", model,
            "--base-url", self._base_url,
            "--api-key-env", self._api_key_env,
            "--max-steps", str(self._max_steps),
            "--max-minutes", str(self._max_minutes),
            "--context-tokens", str(self._context_tokens),
        ]
        # Act as Harbor's own exec would: same directory, same user, same
        # task-wide environment.
        workdir = getattr(environment.task_env_config, "workdir", None)
        if workdir:
            command += ["--workdir", str(workdir)]
        if environment.default_user is not None:
            command += ["--user", str(environment.default_user)]
        merge = getattr(environment, "_merge_env", None)
        for key, value in ((merge(None) if merge else None) or {}).items():
            command += ["--docker-env", f"{key}={value}"]

        self.logs_dir.mkdir(parents=True, exist_ok=True)
        with open(self.logs_dir / "ghost-work.log", "wb") as log:
            proc = await asyncio.create_subprocess_exec(
                *command, stdout=asyncio.subprocess.PIPE, stderr=log
            )
            try:
                report, _ = await proc.communicate()
                (self.logs_dir / "report.json").write_bytes(report)
            finally:
                # Harbor cancels this coroutine at the task's deadline. The
                # loop must not go on working in a container being graded.
                if proc.returncode is None:
                    proc.kill()
                    await proc.wait()
                self._account(state, context)

    def _account(self, state: Path, context: AgentContext) -> None:
        """Token totals come from the state on disk, so a run that was cut
        off is still counted."""
        try:
            usage = json.loads((state / "state.json").read_text())["usage"]
        except (OSError, ValueError, KeyError):
            return
        read, cached, wrote = usage["input"], usage["cached"], usage["output"]
        context.n_input_tokens = read
        context.n_cache_tokens = cached
        context.n_output_tokens = wrote
        context.cost_usd = (
            cached * PRICE["cached"] + (read - cached) * PRICE["fresh"] + wrote * PRICE["output"]
        ) / 1_000_000
