# Plan 10 — Final Testing & Polish

> **Status**: pending  
> **Created**: 2026-06-11  
> **Depends on**: Plan 01-09 (all features)  
> **Goal**: Comprehensive testing, bug fixes, and final polish before release  
> **Agent**: implementation  

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**  
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: End-to-end testing, performance optimization, and UI polish.

**Deliverables**:
1. Full test suite with ≥85% coverage (Rust + TS)
2. Mobile responsive testing (375px, 768px, 1920px)
3. Performance optimization (bundle size, load times)
4. Accessibility audit (basic)
5. Documentation updates
6. Deployment checklist

**Estimated effort**: Medium (1 day)

---

## Context

### Requirements
- All features tested end-to-end
- No regressions from previous plans
- Mobile-first design verified
- Production-ready build

### Testing Checklist
- Backend: `cargo test --all-features`
- Frontend: `npm test`
- E2E: Manual testing on live server
- Lint: `cargo clippy`, `npm run lint`

---

## Step-by-Step Tasks

### T1: Run Full Test Suite

**What to do**:
1. Backend: `cargo test --all-features -- --test-threads=1`
2. Frontend: `npm test -- --coverage`
3. Fix any failing tests
4. Verify ≥85% coverage on both

**Before**: (tests may have gaps)

**After**:
- All tests pass
- Coverage reports generated
- Gaps documented

**Verification**:
- [ ] `cargo test` exits 0
- [ ] `npm test` exits 0
- [ ] Coverage ≥85% on both

---

### T2: Mobile Responsive Testing

**What to do**:
1. Test on 375px (iPhone SE)
2. Test on 768px (iPad)
3. Test on 1920px (desktop)
4. Check touch targets ≥44px
5. Verify no horizontal scroll

**Before**: (may have layout issues)

**After**:
- All breakpoints tested
- Layout issues fixed
- Touch targets verified

**Verification**:
- [ ] 375px: All content accessible
- [ ] 768px: Grid layouts work
- [ ] 1920px: No excessive whitespace

---

### T3: Performance Optimization

**What to do**:
1. Check bundle size: `npm run build && ls -lh dist/`
2. Optimize large components
3. Add lazy loading for routes
4. Check Lighthouse score (if available)

**Before**: (may have large bundle)

**After**:
- Bundle size <500KB (gzipped)
- Lazy loading implemented
- Lighthouse score >80

**Verification**:
- [ ] Bundle size acceptable
- [ ] Lazy loading works
- [ ] Lighthouse score >80

---

### T4: Accessibility Audit

**What to do**:
1. Check contrast ratios (WCAG AA)
2. Add aria-labels to icon buttons
3. Test keyboard navigation
4. Add alt text to images (if any)

**Before**: (may have a11y issues)

**After**:
- Contrast ratios pass
- All interactive elements labeled
- Keyboard navigation works

**Verification**:
- [ ] Contrast ≥4.5:1 for text
- [ ] All buttons have labels
- [ ] Tab navigation works

---

### T5: Documentation Updates

**File**: `README.md`, `docs/`

**What to do**:
1. Update README with setup instructions
2. Add API documentation
3. Add frontend component docs
4. Create deployment guide

**Before**: (documentation incomplete)

**After**:
```markdown
# qctrl

A Rust REST API + React TypeScript frontend for managing a Quake 2 server.

## Quick Start
```bash
cp config.defaults.yaml config.yaml
# Edit config.yaml with your server details
cargo run --bin api
```

## Features
- RCON command execution
- Map management
- Player kick/ban
- Real-time logs
```

**Verification**:
- [ ] README complete
- [ ] API docs generated
- [ ] Deployment guide exists

---

### T6: Bug Fixes & Polish

**What to do**:
1. Fix all identified bugs
2. Add loading states where missing
3. Improve error messages
4. Add tooltips for unclear UI

**Before**: (may have bugs)

**After**:
- All bugs fixed
- Loading states added
- Error messages clear

**Verification**:
- [ ] No known bugs
- [ ] All loading states present
- [ ] Error messages helpful

---

### T7: Deployment Checklist

**File**: `DEPLOYMENT.md`

**What to do**:
1. Create deployment checklist
2. Document production setup
3. Add environment variables
4. Add monitoring recommendations

**After**:
```markdown
# Deployment Checklist

## Pre-deployment
- [ ] All tests pass
- [ ] Lint clean
- [ ] Build succeeds
- [ ] Config updated for production

## Deployment
1. Build: `cargo build --release`
2. Build frontend: `npm run build`
3. Copy binaries to server
4. Set up systemd service
5. Configure reverse proxy (nginx)

## Post-deployment
- [ ] Health check passes
- [ ] All endpoints respond
- [ ] Logs streaming works
```

**Verification**:
- [ ] Checklist complete
- [ ] Steps clear
- [ ] Environment documented

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `README.md` | Update docs | P0 |
| `DEPLOYMENT.md` | New file | P0 |
| All test files | Ensure coverage | P0 |

---

## Verification Checklist

- [ ] T1: All tests pass (backend + frontend)
- [ ] T2: ≥85% coverage on both
- [ ] T3: Mobile responsive verified
- [ ] T4: Performance optimized
- [ ] T5: Accessibility audit passed
- [ ] T6: Documentation complete
- [ ] T7: Deployment checklist ready
- [ ] T8: No known bugs
- [ ] T9: `cargo clippy` clean
- [ ] T10: `npm run lint` clean

---

## Next Steps

After Plan 10 completes:
- Plan 11: Deployment setup (if needed)
- Release v1.0
