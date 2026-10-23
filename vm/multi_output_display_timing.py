"""Fixed Display-test observation budgets; import performs no OS operations.

Snapshot work hashes the complete preserved inventory. Its explicit budgets
are separate from the canonical six-second native command/input contract.
"""

PHASE_SECONDS = {
    'before_preservation': 45,
    'before_supervision': 15,
    'startup_authority': 15,
    'owner_initialization': 90,
    'publish_ready': 10,
    'resource_cleanup': 120,
    'after_preservation': 45,
    'after_supervision': 15,
    'finish_validation': 10,
}
STARTUP_PHASES = ('before_preservation', 'before_supervision', 'startup_authority',
                  'owner_initialization', 'publish_ready')
CLEANUP_PHASES = ('resource_cleanup', 'after_preservation', 'after_supervision', 'finish_validation')
STARTUP_PHASE_SECONDS = sum(PHASE_SECONDS[name] for name in STARTUP_PHASES)
CLEANUP_PHASE_SECONDS = sum(PHASE_SECONDS[name] for name in CLEANUP_PHASES)
STARTUP_READY_SECONDS = 190
CLEANUP_RECEIPT_SECONDS = 210
FINISH_REQUEST_SECONDS = 45
STATE_OBSERVATION_SECONDS = 8
OWNER_LEASE_SECONDS = 480
INTERACTION_SECONDS = 420
UNIT_STOP_SECONDS = 20
UNIT_RUNTIME_SECONDS = 900

assert STARTUP_PHASE_SECONDS + STATE_OBSERVATION_SECONDS < STARTUP_READY_SECONDS
assert CLEANUP_PHASE_SECONDS + STATE_OBSERVATION_SECONDS < CLEANUP_RECEIPT_SECONDS
assert STARTUP_PHASE_SECONDS + OWNER_LEASE_SECONDS + CLEANUP_PHASE_SECONDS + UNIT_STOP_SECONDS < UNIT_RUNTIME_SECONDS
assert INTERACTION_SECONDS < OWNER_LEASE_SECONDS
