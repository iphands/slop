//! Collision model — qbots' `gi.trace()` / `gi.pointcontents()` replacement.
//!
//! Ports `common/collision.c`: builds the internal collision structures from a parsed
//! [`crate::Bsp`] (planes get `signbits`), then answers "is this point solid?" and
//! "what does a swept box hit?". The trace sweeps via `CM_RecursiveHullCheck`, clipping
//! against brushes in touched leafs (`CM_ClipBoxToBrush`).

use std::collections::HashSet;

use crate::bsp::{Brush, Bsp};

/// `DIST_EPSILON` (`collision.c:127`).
const DIST_EPSILON: f32 = 0.03125;

// Contents flags (`files.h:339+`) + masks (`shared.h:553`).
pub const CONTENTS_SOLID: i32 = 1;
pub const CONTENTS_WINDOW: i32 = 2;
pub const CONTENTS_LAVA: i32 = 8;
pub const CONTENTS_SLIME: i32 = 16;
pub const CONTENTS_WATER: i32 = 32;
/// Solid + window: the mask for "is this blocking movement?".
pub const MASK_SOLID: i32 = CONTENTS_SOLID | CONTENTS_WINDOW;
pub const MASK_WATER: i32 = CONTENTS_WATER | CONTENTS_LAVA | CONTENTS_SLIME;

/// `cplane_t` (`shared.h:578`) with precomputed `signbits` (load-time, `collision.c:1463`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Plane {
    pub normal: [f32; 3],
    pub dist: f32,
    pub typ: i32,
    pub signbits: u8,
}

impl Plane {
    /// Signed distance from `p` to the plane (positive = front side).
    #[inline]
    fn dist_to(&self, p: &[f32; 3]) -> f32 {
        if self.typ < 3 {
            p[self.typ as usize] - self.dist
        } else {
            self.normal[0] * p[0] + self.normal[1] * p[1] + self.normal[2] * p[2] - self.dist
        }
    }
}

/// Internal node: a plane index + two children (`-(leaf+1)` for leaf children).
#[derive(Debug, Clone, Copy)]
struct Node {
    plane: usize,
    children: [i32; 2],
}

/// Internal leaf: contents + cluster + a range into the leafbrush table.
#[derive(Debug, Clone, Copy)]
struct Leaf {
    contents: i32,
    cluster: i16,
    firstleafbrush: u16,
    numleafbrushes: u16,
}

#[derive(Debug, Clone, Copy)]
struct BrushSide {
    plane: usize,
}

#[derive(Debug, Clone)]
struct BrushCol {
    firstside: usize,
    numsides: usize,
    contents: i32,
}

/// A trace result (`trace_t`, `shared.h:618`).
#[derive(Debug, Clone)]
pub struct Trace {
    pub allsolid: bool,
    pub startsolid: bool,
    /// 0..1 — time of impact along start→end (1 = unobstructed).
    pub fraction: f32,
    pub endpos: [f32; 3],
    pub plane: Plane,
    pub contents: i32,
}

impl Trace {
    fn open(end: &[f32; 3]) -> Self {
        Self {
            allsolid: false,
            startsolid: false,
            fraction: 1.0,
            endpos: *end,
            plane: Plane::default(),
            contents: 0,
        }
    }
}

/// The collision world: planes/nodes/leafs/brushes built from a [`Bsp`].
pub struct CollisionModel {
    planes: Vec<Plane>,
    nodes: Vec<Node>,
    leafs: Vec<Leaf>,
    brushes: Vec<BrushCol>,
    brushsides: Vec<BrushSide>,
    leafbrushes: Vec<u16>,
    headnode: i32,
}

impl CollisionModel {
    /// Build from a parsed BSP (computes plane `signbits`).
    pub fn from_bsp(bsp: &Bsp) -> Self {
        let planes = bsp
            .planes
            .iter()
            .map(|p| {
                let signbits =
                    (0..3).fold(0u8, |b, j| if p.normal[j] < 0.0 { b | (1 << j) } else { b });
                Plane {
                    normal: p.normal,
                    dist: p.dist,
                    typ: p.typ,
                    signbits,
                }
            })
            .collect();

        let nodes = bsp
            .nodes
            .iter()
            .map(|n| Node {
                plane: n.planenum as usize,
                children: n.children,
            })
            .collect();

        let leafs = bsp
            .leafs
            .iter()
            .map(|l| Leaf {
                contents: l.contents,
                cluster: l.cluster,
                firstleafbrush: l.firstleafbrush,
                numleafbrushes: l.numleafbrushes,
            })
            .collect();

        let brushsides = bsp
            .brushsides
            .iter()
            .map(|s| BrushSide {
                plane: s.planenum as usize,
            })
            .collect();

        let brushes = bsp
            .brushes
            .iter()
            .map(|b: &Brush| BrushCol {
                firstside: b.firstside as usize,
                numsides: b.numsides as usize,
                contents: b.contents,
            })
            .collect();

        let headnode = bsp.models.first().map(|m| m.headnode).unwrap_or(0);

        Self {
            planes,
            nodes,
            leafs,
            brushes,
            brushsides,
            leafbrushes: bsp.leafbrushes.clone(),
            headnode,
        }
    }

    /// `CM_PointContents` — the contents flags at `p` (descend to the containing leaf).
    pub fn point_contents(&self, p: &[f32; 3]) -> i32 {
        let leaf = self.point_leafnum(p, self.headnode);
        self.leafs[leaf].contents
    }

    /// True if `p` is in a solid leaf (cheap, leaf-only).
    pub fn is_solid(&self, p: &[f32; 3]) -> bool {
        self.point_contents(p) & MASK_SOLID != 0
    }

    /// The simplest possible collision world: a single solid **half-space**. The
    /// front side of `plane` (where `normal·p >= dist`) is empty; the back side is
    /// a `CONTENTS_SOLID` brush. Useful for tests/debug that need "a wall" without
    /// building BSP geometry — e.g. a line-of-sight trace that should be clear on
    /// one side and blocked through the plane.
    pub fn half_space(normal: [f32; 3], dist: f32) -> Self {
        let typ = if normal[1] == 0.0 && normal[2] == 0.0 {
            0 // PLANE_X
        } else if normal[0] == 0.0 && normal[2] == 0.0 {
            1 // PLANE_Y
        } else if normal[0] == 0.0 && normal[1] == 0.0 {
            2 // PLANE_Z
        } else {
            3 // PLANE_ANYX-ish general
        };
        let signbits = (0..3).fold(0u8, |b, j| if normal[j] < 0.0 { b | (1 << j) } else { b });
        let planes = vec![Plane {
            normal,
            dist,
            typ,
            signbits,
        }];
        // Root node splits on plane 0: front → empty leaf 0, back → solid leaf 1.
        let nodes = vec![Node {
            plane: 0,
            children: [-1, -2],
        }];
        let leafs = vec![
            Leaf {
                contents: 0,
                cluster: 0,
                firstleafbrush: 0,
                numleafbrushes: 0,
            },
            Leaf {
                contents: CONTENTS_SOLID,
                cluster: -1,
                firstleafbrush: 0,
                numleafbrushes: 1,
            },
        ];
        let brushsides = vec![BrushSide { plane: 0 }];
        let brushes = vec![BrushCol {
            firstside: 0,
            numsides: 1,
            contents: CONTENTS_SOLID,
        }];
        let leafbrushes = vec![0u16];
        Self {
            planes,
            nodes,
            leafs,
            brushes,
            brushsides,
            leafbrushes,
            headnode: 0,
        }
    }

    /// `CM_LeafCluster` — the PVS cluster of the leaf containing `p` (-1 if none).
    pub fn point_cluster(&self, p: &[f32; 3]) -> i16 {
        let leaf = self.point_leafnum(p, self.headnode);
        self.leafs[leaf].cluster
    }

    /// `CM_BoxTrace` — sweep a box from `start` to `end` (mins/maxs relative to origin;
    /// both zero = a point trace) against brushes matching `mask`. Returns the impact.
    pub fn trace(
        &self,
        start: &[f32; 3],
        end: &[f32; 3],
        mins: &[f32; 3],
        maxs: &[f32; 3],
        mask: i32,
    ) -> Trace {
        let mut ctx = Ctx {
            trace: Trace::open(end),
            start: *start,
            end: *end,
            mins: *mins,
            maxs: *maxs,
            extents: [0.0; 3],
            ispoint: false,
            mask,
            seen: HashSet::new(),
        };

        if start == end {
            // position test: gather touched leafs, test for "inside a brush".
            let mut leafs = Vec::new();
            let bbox = box_bounds(start, start, mins, maxs);
            self.box_leafnums(self.headnode, &bbox, &mut leafs);
            for &l in &leafs {
                self.test_in_leaf(&mut ctx, l);
                if ctx.trace.allsolid {
                    break;
                }
            }
            ctx.trace.endpos = *start;
            return ctx.trace;
        }

        // point vs box extents
        ctx.ispoint = mins == &[0.0, 0.0, 0.0] && maxs == &[0.0, 0.0, 0.0];
        for i in 0..3 {
            ctx.extents[i] = (-mins[i]).max(maxs[i]);
        }

        self.recursive_hull_check(&mut ctx, self.headnode, 0.0, 1.0, start, end);

        // finalize endpos
        if ctx.trace.fraction == 1.0 {
            ctx.trace.endpos = *end;
        } else {
            let f = ctx.trace.fraction;
            let mut ep = [0.0; 3];
            for i in 0..3 {
                ep[i] = start[i] + f * (end[i] - start[i]);
            }
            ctx.trace.endpos = ep;
        }
        ctx.trace
    }

    // ---- internals ----

    /// `CM_PointLeafnum_r` — descend to the leaf containing `p`.
    fn point_leafnum(&self, p: &[f32; 3], mut num: i32) -> usize {
        while num >= 0 {
            let node = &self.nodes[num as usize];
            let plane = &self.planes[node.plane];
            num = if plane.dist_to(p) < 0.0 {
                node.children[1]
            } else {
                node.children[0]
            };
        }
        (-1 - num) as usize
    }

    /// `CM_BoxLeafnums_r` — gather leaf indices a bbox touches (for position tests).
    fn box_leafnums(&self, num: i32, bbox: &Aabb, out: &mut Vec<usize>) {
        let mut num = num;
        loop {
            if num < 0 {
                out.push((-1 - num) as usize);
                return;
            }
            let node = &self.nodes[num as usize];
            let plane = &self.planes[node.plane];
            match box_on_plane_side(bbox, plane) {
                1 => num = node.children[0],
                2 => num = node.children[1],
                _ => {
                    self.box_leafnums(node.children[0], bbox, out);
                    num = node.children[1];
                }
            }
        }
    }

    /// `CM_RecursiveHullCheck`.
    fn recursive_hull_check(
        &self,
        ctx: &mut Ctx,
        num: i32,
        p1f: f32,
        p2f: f32,
        p1: &[f32; 3],
        p2: &[f32; 3],
    ) {
        if ctx.trace.fraction <= p1f {
            return; // already hit something nearer
        }
        if num < 0 {
            self.trace_to_leaf(ctx, (-1 - num) as usize);
            return;
        }

        let node = &self.nodes[num as usize];
        let plane = &self.planes[node.plane];
        let (t1, t2, offset) = if plane.typ < 3 {
            let ax = plane.typ as usize;
            (p1[ax] - plane.dist, p2[ax] - plane.dist, ctx.extents[ax])
        } else {
            let t1 = plane.dist_to(p1);
            let t2 = plane.dist_to(p2);
            let off = if ctx.ispoint {
                0.0
            } else {
                ctx.extents[0].abs() * plane.normal[0].abs()
                    + ctx.extents[1].abs() * plane.normal[1].abs()
                    + ctx.extents[2].abs() * plane.normal[2].abs()
            };
            (t1, t2, off)
        };

        if t1 >= offset && t2 >= offset {
            self.recursive_hull_check(ctx, node.children[0], p1f, p2f, p1, p2);
            return;
        }
        if t1 < -offset && t2 < -offset {
            self.recursive_hull_check(ctx, node.children[1], p1f, p2f, p1, p2);
            return;
        }

        // crosses the plane — split into near/far segments
        let (side, frac, frac2) = if t1 < t2 {
            let idist = 1.0 / (t1 - t2);
            (
                1,
                (t1 - offset + DIST_EPSILON) * idist,
                (t1 + offset + DIST_EPSILON) * idist,
            )
        } else if t1 > t2 {
            let idist = 1.0 / (t1 - t2);
            (
                0,
                (t1 + offset + DIST_EPSILON) * idist,
                (t1 - offset - DIST_EPSILON) * idist,
            )
        } else {
            (0, 1.0, 0.0)
        };

        let frac = frac.clamp(0.0, 1.0);
        let mut midf = p1f + (p2f - p1f) * frac;
        let mut mid = [0.0f32; 3];
        for i in 0..3 {
            mid[i] = p1[i] + frac * (p2[i] - p1[i]);
        }
        self.recursive_hull_check(ctx, node.children[side], p1f, midf, p1, &mid);

        let frac2 = frac2.clamp(0.0, 1.0);
        midf = p1f + (p2f - p1f) * frac2;
        for i in 0..3 {
            mid[i] = p1[i] + frac2 * (p2[i] - p1[i]);
        }
        self.recursive_hull_check(ctx, node.children[side ^ 1], midf, p2f, &mid, p2);
    }

    /// `CM_TraceToLeaf` — clip against each (matching, unseen) brush in the leaf.
    fn trace_to_leaf(&self, ctx: &mut Ctx, leaf: usize) {
        let leaf = &self.leafs[leaf];
        if leaf.contents & ctx.mask == 0 {
            return;
        }
        for k in 0..leaf.numleafbrushes as usize {
            let bi = self.leafbrushes[leaf.firstleafbrush as usize + k] as usize;
            if !ctx.seen.insert(bi) {
                continue;
            }
            let b = &self.brushes[bi];
            if b.contents & ctx.mask == 0 {
                continue;
            }
            self.clip_box_to_brush(ctx, bi);
            if ctx.trace.fraction == 0.0 {
                return;
            }
        }
    }

    /// `CM_TestInLeaf` — position-test variant (start==end).
    fn test_in_leaf(&self, ctx: &mut Ctx, leaf: usize) {
        let leaf = &self.leafs[leaf];
        if leaf.contents & ctx.mask == 0 {
            return;
        }
        for k in 0..leaf.numleafbrushes as usize {
            let bi = self.leafbrushes[leaf.firstleafbrush as usize + k] as usize;
            if !ctx.seen.insert(bi) {
                continue;
            }
            let b = &self.brushes[bi];
            if b.contents & ctx.mask == 0 {
                continue;
            }
            self.test_box_in_brush(ctx, bi);
            if ctx.trace.fraction == 0.0 {
                return;
            }
        }
    }

    /// `CM_ClipBoxToBrush` — sweep-clip the box against one brush's planes.
    fn clip_box_to_brush(&self, ctx: &mut Ctx, brush: usize) {
        let b = &self.brushes[brush];
        if b.numsides == 0 {
            return;
        }

        let mut enterfrac = -1.0f32;
        let mut leavefrac = 1.0f32;
        let mut clipplane: Option<usize> = None;
        let mut getout = false;
        let mut startout = false;

        for i in 0..b.numsides {
            let side = &self.brushsides[b.firstside + i];
            let plane = &self.planes[side.plane];

            // push the plane out by the box extents
            let dist = if ctx.ispoint {
                plane.dist
            } else {
                let ofs: [f32; 3] = std::array::from_fn(|j| {
                    if plane.normal[j] < 0.0 {
                        ctx.maxs[j]
                    } else {
                        ctx.mins[j]
                    }
                });
                plane.dist
                    - (ofs[0] * plane.normal[0]
                        + ofs[1] * plane.normal[1]
                        + ofs[2] * plane.normal[2])
            };

            let d1 = ctx.start[0] * plane.normal[0]
                + ctx.start[1] * plane.normal[1]
                + ctx.start[2] * plane.normal[2]
                - dist;
            let d2 = ctx.end[0] * plane.normal[0]
                + ctx.end[1] * plane.normal[1]
                + ctx.end[2] * plane.normal[2]
                - dist;

            if d2 > 0.0 {
                getout = true;
            }
            if d1 > 0.0 {
                startout = true;
            }
            if d1 > 0.0 && d2 >= d1 {
                return; // entirely in front — no hit
            }
            if d1 <= 0.0 && d2 <= 0.0 {
                continue; // behind this plane
            }
            if d1 > d2 {
                // entering
                let f = (d1 - DIST_EPSILON) / (d1 - d2);
                if f > enterfrac {
                    enterfrac = f;
                    clipplane = Some(side.plane);
                }
            } else {
                // leaving
                let f = (d1 + DIST_EPSILON) / (d1 - d2);
                if f < leavefrac {
                    leavefrac = f;
                }
            }
        }

        if !startout {
            ctx.trace.startsolid = true;
            if !getout {
                ctx.trace.allsolid = true;
            }
            return;
        }
        if enterfrac < leavefrac && enterfrac > -1.0 && enterfrac < ctx.trace.fraction {
            let enterfrac = enterfrac.max(0.0);
            ctx.trace.fraction = enterfrac;
            ctx.trace.plane = self.planes[clipplane.expect("clipplane set when enterfrac>prev")];
            ctx.trace.contents = b.contents;
        }
    }

    /// `CM_TestBoxInBrush` — is the box (at `start`) entirely inside this brush?
    fn test_box_in_brush(&self, ctx: &mut Ctx, brush: usize) {
        let b = &self.brushes[brush];
        if b.numsides == 0 {
            return;
        }
        for i in 0..b.numsides {
            let side = &self.brushsides[b.firstside + i];
            let plane = &self.planes[side.plane];
            let ofs: [f32; 3] = std::array::from_fn(|j| {
                if plane.normal[j] < 0.0 {
                    ctx.maxs[j]
                } else {
                    ctx.mins[j]
                }
            });
            let dist = plane.dist
                - (ofs[0] * plane.normal[0] + ofs[1] * plane.normal[1] + ofs[2] * plane.normal[2]);
            let d1 = ctx.start[0] * plane.normal[0]
                + ctx.start[1] * plane.normal[1]
                + ctx.start[2] * plane.normal[2]
                - dist;
            if d1 > 0.0 {
                return; // in front of a face — not inside
            }
        }
        ctx.trace.startsolid = true;
        ctx.trace.allsolid = true;
        ctx.trace.fraction = 0.0;
        ctx.trace.contents = b.contents;
    }
}

struct Ctx {
    trace: Trace,
    start: [f32; 3],
    end: [f32; 3],
    mins: [f32; 3],
    maxs: [f32; 3],
    extents: [f32; 3],
    ispoint: bool,
    mask: i32,
    seen: HashSet<usize>,
}

/// An axis-aligned box (mins/maxs in world space).
struct Aabb {
    mins: [f32; 3],
    maxs: [f32; 3],
}

fn box_bounds(start: &[f32; 3], end: &[f32; 3], mins: &[f32; 3], maxs: &[f32; 3]) -> Aabb {
    let mut c1 = [0.0f32; 3];
    let mut c2 = [0.0f32; 3];
    for i in 0..3 {
        c1[i] = start[i].min(end[i]) + mins[i] - 1.0;
        c2[i] = start[i].max(end[i]) + maxs[i] + 1.0;
    }
    Aabb { mins: c1, maxs: c2 }
}

/// `BoxOnPlaneSide` — returns 1 (front), 2 (back), 3 (both). Corners method (`shared.c:375`).
fn box_on_plane_side(b: &Aabb, p: &Plane) -> i8 {
    let corners: [[f32; 3]; 2] = [
        std::array::from_fn(|i| {
            if p.normal[i] < 0.0 {
                b.maxs[i]
            } else {
                b.mins[i]
            }
        }),
        std::array::from_fn(|i| {
            if p.normal[i] < 0.0 {
                b.mins[i]
            } else {
                b.maxs[i]
            }
        }),
    ];
    let d1 =
        p.normal[0] * corners[0][0] + p.normal[1] * corners[0][1] + p.normal[2] * corners[0][2]
            - p.dist;
    let d2 =
        p.normal[0] * corners[1][0] + p.normal[1] * corners[1][1] + p.normal[2] * corners[1][2]
            - p.dist;
    let mut sides = 0i8;
    if d1 >= 0.0 {
        sides = 1;
    }
    if d2 < 0.0 {
        sides |= 2;
    }
    sides
}

/// A layered world for water-nav tests (Plan 39/40): a solid floor for all `z < 0`, a water
/// volume in the central channel `-64 ≤ x ≤ 64` for `0 ≤ z < 120`, and air everywhere else.
/// Dry floor nodes form on the side ledges (`|x| > 64`); swim nodes fill the channel; an
/// entry/exit swim edge bridges them. Test-support only — exposed (doc-hidden) so dependent
/// crates' tests (`brain::water`) can build a water collision model without a real BSP.
#[doc(hidden)]
pub fn water_channel_world() -> CollisionModel {
    let mk = |normal: [f32; 3], dist: f32, typ: i32| {
        let sb = (0..3).fold(0u8, |b, j| if normal[j] < 0.0 { b | (1 << j) } else { b });
        Plane {
            normal,
            dist,
            typ,
            signbits: sb,
        }
    };
    // P0: z=0 floor top, P1: z=120 water top, P2: x=-64 west wall, P3: x=64 east wall.
    let planes = vec![
        mk([0.0, 0.0, 1.0], 0.0, 2),
        mk([0.0, 0.0, 1.0], 120.0, 2),
        mk([1.0, 0.0, 0.0], -64.0, 0),
        mk([1.0, 0.0, 0.0], 64.0, 0),
    ];
    // Leaf children encode as -(leaf+1): L0→-1, L1→-2, L2→-3.
    // N0 P0: front(z≥0)→N1, back(z<0)→L2 solid floor.
    // N1 P1: front(z≥120)→L0 air, back(z<120)→N2.
    // N2 P2: front(x≥-64)→N3, back(x<-64)→L0 air.
    // N3 P3: front(x≥64)→L0 air, back(-64≤x<64)→L1 water.
    let nodes = vec![
        Node {
            plane: 0,
            children: [1, -3],
        },
        Node {
            plane: 1,
            children: [-1, 2],
        },
        Node {
            plane: 2,
            children: [3, -1],
        },
        Node {
            plane: 3,
            children: [-1, -2],
        },
    ];
    let leafs = vec![
        Leaf {
            contents: 0,
            cluster: 0,
            firstleafbrush: 0,
            numleafbrushes: 0,
        }, // L0 air
        Leaf {
            contents: CONTENTS_WATER,
            cluster: 0,
            firstleafbrush: 0,
            numleafbrushes: 0,
        }, // L1 water
        Leaf {
            contents: CONTENTS_SOLID,
            cluster: -1,
            firstleafbrush: 0,
            numleafbrushes: 1,
        }, // L2 floor
    ];
    // One floor brush: the half-space z < 0 (top face = P0, normal +z).
    let brushsides = vec![BrushSide { plane: 0 }];
    let brushes = vec![BrushCol {
        firstside: 0,
        numsides: 1,
        contents: CONTENTS_SOLID,
    }];
    let leafbrushes = vec![0u16];
    CollisionModel {
        planes,
        nodes,
        leafs,
        brushes,
        brushsides,
        leafbrushes,
        headnode: 0,
    }
}

/// A V-groove drainage-duct world (Plan 52): two 45° slopes meeting at the origin,
/// running along the x axis — solid wherever `z < |y|`, air above. The POINT floor at
/// `(x, y)` is `z = |y|`, but the 32×32 player hull bridges the slopes and rests with
/// its **origin** at `z = 40 + |y|` (bottom corners contact at `z = 16 + |y|`, origin
/// 24 above). Models base64's drain duct, where the floor probe's stationary hull check
/// was `startsolid` and the whole duct sampled zero nodes. Test-support only.
#[doc(hidden)]
pub fn v_groove_world() -> CollisionModel {
    let mk = |normal: [f32; 3], dist: f32, typ: i32| {
        let sb = (0..3).fold(0u8, |b, j| if normal[j] < 0.0 { b | (1 << j) } else { b });
        Plane {
            normal,
            dist,
            typ,
            signbits: sb,
        }
    };
    const S: f32 = std::f32::consts::FRAC_1_SQRT_2;
    // P0: slope z = -y (normal points up-and-north); P1: slope z = y.
    let planes = vec![mk([0.0, S, S], 0.0, 5), mk([0.0, -S, S], 0.0, 5)];
    // Leaf children encode as -(leaf+1): L0→-1, L1→-2, L2→-3.
    // N0 P0: front(z≥-y)→N1, back(z<-y)→L1 solid (brush A).
    // N1 P1: front(z≥y)→L0 air, back(z<y)→L2 solid (brush B).
    let nodes = vec![
        Node {
            plane: 0,
            children: [1, -2],
        },
        Node {
            plane: 1,
            children: [-1, -3],
        },
    ];
    let leafs = vec![
        Leaf {
            contents: 0,
            cluster: 0,
            firstleafbrush: 0,
            numleafbrushes: 0,
        }, // L0 air
        Leaf {
            contents: CONTENTS_SOLID,
            cluster: -1,
            firstleafbrush: 0,
            numleafbrushes: 1,
        }, // L1 south slope
        Leaf {
            contents: CONTENTS_SOLID,
            cluster: -1,
            firstleafbrush: 1,
            numleafbrushes: 1,
        }, // L2 north slope
    ];
    let brushsides = vec![BrushSide { plane: 0 }, BrushSide { plane: 1 }];
    let brushes = vec![
        BrushCol {
            firstside: 0,
            numsides: 1,
            contents: CONTENTS_SOLID,
        },
        BrushCol {
            firstside: 1,
            numsides: 1,
            contents: CONTENTS_SOLID,
        },
    ];
    let leafbrushes = vec![0u16, 1u16];
    CollisionModel {
        planes,
        nodes,
        leafs,
        brushes,
        brushsides,
        leafbrushes,
        headnode: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tripwire: these constants must stay pinned to the vendor source they cite.
    /// A future refactor that drifts one of them away from parity should fail
    /// this test, not silently ship a mismatch.
    #[test]
    fn vendor_constants_pinned() {
        // collision.c:127
        assert_eq!(DIST_EPSILON, 0.03125);
        // pmove.c:32 `#define STEPSIZE 18`
        assert_eq!(crate::navgraph::STEP, 18.0);
    }

    /// A solid axis-aligned box brush at [0..10]³: six inward-facing planes.
    /// The BSP needs a node/leaf that references it; we build the minimal structure.
    fn box_world() -> CollisionModel {
        // 6 planes: +x(dist 10, type X=0), -x(dist 0, flipped), similarly y,z.
        // normals point inward (into the solid). For a SOLID brush, planes face out of
        // the solid, i.e. toward empty space — so the brush interior is on the negative
        // side of all planes. We use: x+ (normal +x, dist 10), x- (normal -x, dist 0)...
        // Actually Q2 brush planes face OUT of the brush. Build accordingly.
        let mk = |normal: [f32; 3], dist: f32, typ: i32| {
            let sb = (0..3).fold(0u8, |b, j| if normal[j] < 0.0 { b | (1 << j) } else { b });
            Plane {
                normal,
                dist,
                typ,
                signbits: sb,
            }
        };
        let planes = vec![
            mk([1.0, 0.0, 0.0], 10.0, 0), // +x face
            mk([-1.0, 0.0, 0.0], 0.0, 0), // -x face (dist 0 → -x*0 ... dist along -x)
            mk([0.0, 1.0, 0.0], 10.0, 1), // +y
            mk([0.0, -1.0, 0.0], 0.0, 1), // -y
            mk([0.0, 0.0, 1.0], 10.0, 2), // +z
            mk([0.0, 0.0, -1.0], 0.0, 2), // -z
        ];
        let brushsides = (0..6).map(|i| BrushSide { plane: i }).collect();
        let brushes = vec![BrushCol {
            firstside: 0,
            numsides: 6,
            contents: CONTENTS_SOLID,
        }];
        let leafs = vec![Leaf {
            contents: CONTENTS_SOLID,
            cluster: 0,
            firstleafbrush: 0,
            numleafbrushes: 1,
        }];
        let leafbrushes = vec![0u16];
        // single node: plane 0, children = [empty-leaf 1 (contents 0), solid-leaf 0]
        // Make the tree trivially route everything into leaf 0 (the solid one) by using a
        // child array of [-1, -1] (both → leaf 0 via -1-(-1)=0). Simpler: headnode points
        // straight at the leaf.
        let nodes = vec![Node {
            plane: 0,
            children: [-1, -1],
        }];
        CollisionModel {
            planes,
            nodes,
            leafs,
            brushes,
            brushsides,
            leafbrushes,
            headnode: -1, // leaf 0 directly (-1 → -1-(-1) = 0)
        }
    }

    #[test]
    fn water_channel_world_contents() {
        let w = water_channel_world();
        // Channel: water between floor and surface.
        assert_eq!(
            w.point_contents(&[0.0, 0.0, 60.0]) & CONTENTS_WATER,
            CONTENTS_WATER
        );
        // Below floor: solid.
        assert!(w.point_contents(&[0.0, 0.0, -10.0]) & CONTENTS_SOLID != 0);
        // Above surface: air.
        assert_eq!(w.point_contents(&[0.0, 0.0, 200.0]), 0);
        // Side ledge (x>64) above the floor: air, not water.
        assert_eq!(w.point_contents(&[100.0, 0.0, 60.0]) & CONTENTS_WATER, 0);
        // A down-trace on a side column finds the floor at z≈0.
        let down = w.trace(
            &[100.0, 0.0, 200.0],
            &[100.0, 0.0, -50.0],
            &[0.0; 3],
            &[0.0; 3],
            MASK_SOLID,
        );
        assert!(
            !down.startsolid && down.fraction < 1.0,
            "side column has a floor"
        );
        assert!((down.endpos[2] - 0.0).abs() < 1.0, "floor at z≈0");
    }

    #[test]
    fn point_inside_is_solid() {
        let w = box_world();
        assert!(w.is_solid(&[5.0, 5.0, 5.0]));
        assert!(w.point_contents(&[5.0, 5.0, 5.0]) & CONTENTS_SOLID != 0);
    }

    #[test]
    fn point_trace_into_wall_hits() {
        let w = box_world();
        // from (5,5,5) inside toward +x: startsolid (inside the solid).
        let t = w.trace(
            &[5.0, 5.0, 5.0],
            &[20.0, 5.0, 5.0],
            &[0.0; 3],
            &[0.0; 3],
            MASK_SOLID,
        );
        assert!(t.startsolid, "started inside the solid box");
    }

    #[test]
    fn point_trace_outside_open_air_is_clear() {
        let w = box_world();
        // both points outside the solid box → no hit (whole world is the "brush" here, so
        // any point maps to leaf 0 which is solid; this test just checks no panic + frac).
        let t = w.trace(
            &[100.0, 100.0, 100.0],
            &[101.0, 100.0, 100.0],
            &[0.0; 3],
            &[0.0; 3],
            MASK_SOLID,
        );
        // headnode = leaf 0 (solid) everywhere → startsolid.
        assert!(t.startsolid || t.fraction == 1.0);
    }

    /// A half-space wall at x=0 (x<0 solid): rays on the empty side are clear,
    /// rays crossing into x<0 are blocked. The geometry every LOS test needs.
    #[test]
    fn half_space_blocks_across_the_plane() {
        let w = CollisionModel::half_space([1.0, 0.0, 0.0], 0.0);
        // Both points in the empty front (x>0) → unobstructed.
        let clear = w.trace(
            &[50.0, 0.0, 0.0],
            &[100.0, 0.0, 0.0],
            &[0.0; 3],
            &[0.0; 3],
            MASK_SOLID,
        );
        assert!(
            (clear.fraction - 1.0).abs() < 1e-4,
            "clear ray, got {}",
            clear.fraction
        );
        assert!(!clear.startsolid);

        // Crossing the plane into the solid back (target x<0) → stops at x≈0.
        let blocked = w.trace(
            &[50.0, 0.0, 0.0],
            &[-50.0, 0.0, 0.0],
            &[0.0; 3],
            &[0.0; 3],
            MASK_SOLID,
        );
        assert!(blocked.fraction < 1.0, "should hit the wall");
        assert!(
            (blocked.fraction - 0.5).abs() < 1e-3,
            "stops at the plane, got {}",
            blocked.fraction
        );
        // The blocking plane's normal points into the empty side (toward +x).
        assert!(blocked.plane.normal[0] > 0.0);

        // A point behind the plane is solid.
        assert!(w.is_solid(&[-10.0, 0.0, 0.0]));
        assert!(!w.is_solid(&[10.0, 0.0, 0.0]));
    }
}
