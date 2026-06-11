# Plan 04 — Deathmatch Controls UI

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 03 (Frontend scaffolding)  
> **Goal**: Implement UI for deathmatch settings: dmflags, timelimit, fraglimit, and map restart  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Create mobile-friendly controls for deathmatch server settings with instant feedback.

**Deliverables**:
1. dmflags selector with common presets (checkboxes for individual flags)
2. timelimit input with validation
3. fraglimit input with validation
4. "Restart Map" button
5. All controls send RCON commands and show success/failure feedback
6. Unit tests for all components

**Estimated effort**: Medium (1 day)

---

## Context

### Requirements
- Mobile-first: large touch targets, no typing when possible
- Immediate feedback: show command being sent, response received
- dmflags: bitmap variable (17424 currently set on server)
- timelimit: minutes (integer, 0 = unlimited)
- fraglimit: score (integer, 0 = unlimited)

### dmflags Bitmasks (from vendor/quakeiicom.html)
| Value | Flag |
|-------|------|
| 1 | No Health |
| 2 | No Powerups |
| 4 | Weapons Stay |
| 8 | No Falling Damage |
| 16 | Instant Powerups |
| 32 | Same Map |
| 64 | Teams by Skin |
| 128 | Teams by Model |
| 256 | No Friendly Fire |
| 512 | Spawn Farthest |
| 1024 | Force Respawn |
| 2048 | No Armor |
| 4096 | Allow Exit |
| 8192 | Infinite Ammo |
| 16384 | Quad Drop |
| 32768 | Fixed FOV |

Current server: `dmflags 17424` = 16384 + 8192 + 2048 + 512 + 256 + 32 + 16 + 4 + 8 + 2 + 1

### RCON Commands
- `dmflags <value>`
- `timelimit <minutes>`
- `fraglimit <score>`
- `map <mapname>` (restarts current map)

---

## Step-by-Step Tasks

### T1: Create dmflags Presets Component

**File**: `frontend/src/components/DmflagsPreset.tsx`

**What to do**:
1. Define common dmflags presets:
   - "Standard" (16 - Instant Powerups)
   - "No Armor" (2048 + 16 = 2064)
   - "Weapons Stay" (4 + 16 = 20)
   - "Full Game" (17424 - current server setting)
2. Create selectable preset cards
3. Show current value display
4. On select, send `dmflags <value>` via RCON

**Before**: (no dmflags UI)

**After**:
```tsx
const PRESETS = [
  { name: "Standard", value: 16, description: "Instant powerups" },
  { name: "Weapons Stay", value: 20, description: "Weapons remain after pickup" },
  { name: "No Armor", value: 2064, description: "No armor, instant powerups" },
  { name: "Full Game", value: 17424, description: "Current server setting" },
];

function DmflagsPreset() {
  const { mutate: execute } = useMutation({ mutationFn: executeRcon });
  
  return (
    <div className="grid grid-cols-2 gap-4">
      {PRESETS.map(preset => (
        <Card key={preset.name} onClick={() => execute(`dmflags ${preset.value}`)}>
          <CardTitle>{preset.name}</CardTitle>
          <CardDescription>{preset.description}</CardDescription>
        </Card>
      ))}
    </div>
  );
}
```

**Tests**:
- Presets render correctly
- Clicking sends correct RCON command
- Shows loading/error states

---

### T2: Create dmflags Bitmask Selector

**File**: `frontend/src/components/DmflagsBits.tsx`

**What to do**:
1. Create checkboxes for each dmflags bit (16 options)
2. Show current combined value
3. On toggle, recalculate and send new value
4. Group related flags (e.g., "Team Settings", "Item Settings")
5. Show "Custom" when value doesn't match presets

**Before**: (no bitmask UI)

**After**:
```tsx
const FLAGS = [
  { bit: 1, name: "No Health" },
  { bit: 2, name: "No Powerups" },
  { bit: 4, name: "Weapons Stay" },
  // ... all 16 flags
];

function DmflagsBits({ currentValue }: { currentValue: number }) {
  const [selected, setSelected] = useState(currentValue);
  
  const toggleBit = (bit: number) => {
    setSelected(prev => prev ^ bit); // Toggle bit
    executeRcon(`dmflags ${selected ^ bit}`);
  };
  
  return (
    <div className="grid grid-cols-2 gap-2">
      {FLAGS.map(flag => (
        <Checkbox
          key={flag.bit}
          checked={currentValue & flag.bit}
          onChange={() => toggleBit(flag.bit)}
          label={flag.name}
        />
      ))}
    </div>
  );
}
```

**Tests**:
- Checkboxes reflect current value
- Toggling sends correct command
- Value calculation is accurate

---

### T3: Create Timelimit Control

**File**: `frontend/src/components/TimelimitControl.tsx`

**What to do**:
1. Input field for minutes (number, min=0, max=999)
2. Quick-select buttons: 15, 30, 45, 60 minutes
3. "Unlimited" button (sets to 0)
4. Validation: reject negative, non-integer
5. Show current value from server (poll via `/rcon status`)

**Before**: (no timelimit UI)

**After**:
```tsx
function TimelimitControl() {
  const [value, setValue] = useState(20);
  const { mutate: execute } = useMutation({ mutationFn: executeRcon });
  
  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (value >= 0 && value <= 999) {
      execute(`timelimit ${value}`);
    }
  };
  
  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <Input
        type="number"
        min={0}
        max={999}
        value={value}
        onChange={e => setValue(Number(e.target.value))}
        placeholder="Minutes (0 = unlimited)"
      />
      <div className="flex gap-2">
        {[15, 30, 45, 60].map(m => (
          <Button key={m} type="button" onClick={() => { setValue(m); }}>
            {m} min
          </Button>
        ))}
        <Button type="button" variant="outline" onClick={() => { setValue(0); }}>
          Unlimited
        </Button>
      </div>
    </form>
  );
}
```

**Tests**:
- Input validates correctly
- Quick-select sets value
- Submit sends correct command

---

### T4: Create Fraglimit Control

**File**: `frontend/src/components/FraglimitControl.tsx`

**What to do**:
1. Similar to timelimit but for frag score
2. Quick-select: 10, 25, 50, 100 frags
3. "Unlimited" button (sets to 0)
4. Validation: non-negative integer

**Before**: (no fraglimit UI)

**After**: (similar to TimelimitControl)

**Tests**:
- Same as TimelimitControl

---

### T5: Create Restart Map Button

**File**: `frontend/src/components/RestartMap.tsx`

**What to do**:
1. Large, prominent button
2. Show loading state while command executes
3. Show success/error feedback
4. Optional: fetch current map from `/rcon status` and display

**Before**: (no restart UI)

**After**:
```tsx
function RestartMap() {
  const { mutate: execute, isPending } = useMutation({ mutationFn: executeRcon });
  
  return (
    <Button
      onClick={() => execute("map q2dm1")} // TODO: get current map
      disabled={isPending}
      className="w-full h-14 text-lg"
    >
      {isPending ? "Restarting..." : "Restart Current Map"}
    </Button>
  );
}
```

**Tests**:
- Button sends correct command
- Loading state shows during execution
- Error handling works

---

### T6: Create Deathmatch Controls Page

**File**: `frontend/src/pages/Deathmatch.tsx`

**What to do**:
1. Combine all controls into single page
2. Section headers for each control group
3. Responsive layout (stack on mobile, side-by-side on desktop)
4. Add to main navigation

**Before**: (no deathmatch page)

**After**:
```tsx
function Deathmatch() {
  return (
    <div className="space-y-6">
      <Section title="Deathmatch Flags">
        <DmflagsPreset />
        <DmflagsBits />
      </Section>
      <Section title="Time Limit">
        <TimelimitControl />
      </Section>
      <Section title="Frag Limit">
        <FraglimitControl />
      </Section>
      <Section title="Map">
        <RestartMap />
      </Section>
    </div>
  );
}
```

**Tests**:
- All sections render
- Navigation works
- Mobile layout stacks correctly

---

### T7: Add Unit Tests

**File**: `frontend/src/components/*.test.tsx`

**What to do**:
1. Test each component with various inputs
2. Test RCON command generation
3. Test error handling
4. Test loading states

**Tests**:
- All tests pass
- ≥85% coverage on new components

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `frontend/src/components/DmflagsPreset.tsx` | New component | P0 |
| `frontend/src/components/DmflagsBits.tsx` | New component | P0 |
| `frontend/src/components/TimelimitControl.tsx` | New component | P0 |
| `frontend/src/components/FraglimitControl.tsx` | New component | P0 |
| `frontend/src/components/RestartMap.tsx` | New component | P0 |
| `frontend/src/pages/Deathmatch.tsx` | New page | P0 |

---

## Open Questions / Risks

1. **Current Map**: How to get current map? May need `/rcon status` parsing or separate endpoint.
2. **Command Feedback**: Should we show raw RCON response or just success/failure? (Recommend: brief success + raw on expand)
3. **Polling**: Should we auto-refresh current settings? (Recommend: manual refresh button first)

---

## Verification Checklist

- [ ] T1: Dmflags presets render and send correct commands
- [ ] T2: Dmflags bitmask toggles work correctly
- [ ] T3: Timelimit input validates and sends commands
- [ ] T4: Fraglimit input validates and sends commands
- [ ] T5: Restart map button sends correct command
- [ ] T6: Deathmatch page combines all controls
- [ ] T7: Mobile layout stacks correctly (375px)
- [ ] T8: `npm test` passes with ≥85% coverage
- [ ] T9: `npm run lint` passes with zero warnings

---

## Next Steps

After Plan 04 completes:
- Plan 05: Map selection UI
- Plan 06: Player management (kick/ban)
- Plan 07: Real-time log streaming
