# Chains

Chains let you compose multiple operations into workflows. Build sequences of operations that execute in order, with the ability to branch and parallelize.

## What's a Chain?

A chain is a graph of operations:

- **Elements** - The operations to run
- **Connections** - How elements flow from one to the next
- **Session Groups** - Which elements share a session

Chains execute operations in dependency order, running parallel paths when possible.

## Creating Chains

### Visual Builder

Praxis includes a visual chain builder using React Flow:

1. Go to **Operations** → **Library**
2. Click **New Chain**
3. Drag operations onto the canvas
4. Connect outputs to inputs
5. Configure session groups
6. Save the chain

### Chain Structure

A chain consists of:

**Start Node** - Entry point, begins execution

**Operation Nodes** - Each runs a semantic operation

**Connections** - Define execution order and data flow

**End** - Implicit when all paths complete

## Session Groups

Session groups control how sessions are managed across operations.

**Same Session Group** - Operations share a session:
- First operation creates the session
- Subsequent operations reuse it
- Session closes after last operation

**Different Session Groups** - Operations get their own sessions:
- Each operation creates and closes its own
- No shared context
- Clean isolation

Why does this matter? Agent sessions have context. If one operation sets something up, the next operation in the same session sees that state.

## Execution

### Running a Chain

1. Select a chain from the library
2. Click **Run**
3. Choose node and agent
4. Watch execution progress

### Execution Flow

The executor:

1. Builds a dependency graph from connections
2. Finds operations with no dependencies (starting points)
3. Executes ready operations (possibly in parallel)
4. Marks completed, finds newly ready operations
5. Repeats until all complete or one fails

### Parallel Execution

Operations without dependencies on each other can run simultaneously. The executor identifies these and runs them in parallel.

```
    ┌─────┐
    │Start│
    └──┬──┘
       │
   ┌───┴───┐
   │       │
┌──▼──┐ ┌──▼──┐
│Op A │ │Op B │  ← These run in parallel
└──┬──┘ └──┬──┘
   │       │
   └───┬───┘
       │
    ┌──▼──┐
    │Op C │  ← This waits for both A and B
    └─────┘
```

## Monitoring Chains

Chain executions appear in the Runs tab alongside individual operations:

- **Chain Name** - Which chain is running
- **Status** - Running, Completed, Failed
- **Progress** - Which elements have completed

Click a chain execution to see:
- Individual element status
- Output from each operation
- Timing information

## Implicit Chains

When you run a single operation, Praxis actually creates an "implicit chain" internally. This provides consistent execution behavior whether you're running one operation or many.

## Chain Cancellation

You can cancel a running chain:

1. Find it in Runs
2. Click **Cancel**

Cancellation:
- Stops queuing new operations
- Lets running operations complete (or cancels them)
- Marks the chain as cancelled

## Use Cases

### Sequential Operations

Run operations in order, each building on the previous:

1. Enumerate capabilities →
2. Identify target →
3. Execute action →
4. Verify result

### Parallel Reconnaissance

Run multiple recon operations simultaneously:

```
     Start
       │
   ┌───┼───┐
   ▼   ▼   ▼
 Enum Tools  Enum Config  Enum Sessions
   │   │   │
   └───┼───┘
       ▼
    Analyze
```

### Staged Operations

Build up context across operations with shared sessions:

1. Establish access (Session A)
2. Gather info (Session A)
3. Take action (Session A)
4. Clean up (Session A)

All in the same session, maintaining context.

## Best Practices

**Plan session groups carefully.** Shared sessions maintain context but also accumulate state. Fresh sessions are cleaner but lose context.

**Handle failures.** If an operation fails, the chain stops. Design for this-put critical operations early.

**Test incrementally.** Run individual operations first, then combine into chains.

**Keep chains focused.** One chain, one goal. Compose larger workflows from smaller chains.

## Troubleshooting

### Chain stuck

- Check which element is running
- Verify that element's operation works alone
- Look at element output for errors

### Wrong execution order

- Verify connections in the visual builder
- Check that dependencies are correct

### Session issues

- Review session group assignments
- Try with separate session groups to isolate
