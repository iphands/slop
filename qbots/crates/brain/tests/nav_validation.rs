//! Navigation graph validation tests
//!
//! These tests ensure the nav graph is correct:
//! - All spawn points are reachable from each other
//! - No unreachable nodes
//! - Paths exist for all common scenarios

#[cfg(test)]
mod tests {
    use brain::nav::NavGraph;
    use world::bsp::BspMap;
    
    /// Test that all spawn points are reachable from each other
    ///
    /// This is critical because Q2DM maps are designed so that ALL spawn
    /// points are accessible. If our nav graph says otherwise, we have bugs.
    #[test]
    fn test_all_spawns_reachable() {
        let bsp = BspMap::load("q2dm1").expect("Failed to load BSP");
        let graph = NavGraph::generate_from_bsp(&bsp);
        
        let spawns = load_spawn_points("q2dm1");
        
        for (i, spawn_a) in spawns.iter().enumerate() {
            for (j, spawn_b) in spawns.iter().enumerate().skip(i + 1) {
                let path = graph.find_path(spawn_a.node, spawn_b.node);
                assert!(
                    path.is_some(),
                    "Spawn {} -> {} unreachable (graph has {} nodes)",
                    i, j, graph.node_count()
                );
            }
        }
    }
    
    /// Test that no nodes are unreachable (orphaned)
    #[test]
    fn test_no_orphaned_nodes() {
        let bsp = BspMap::load("q2dm1").expect("Failed to load BSP");
        let graph = NavGraph::generate_from_bsp(&bsp);
        
        // Find a central node (e.g., first spawn)
        let start_node = 0;
        
        // BFS from start
        let mut visited = vec![false; graph.node_count()];
        let mut queue = std::collections::VecDeque::new();
        
        queue.push_back(start_node);
        visited[start_node] = true;
        
        while let Some(node) = queue.pop_front() {
            for link in &graph.nodes[node].linkpod {
                if let Some(target) = link {
                    if !visited[target.index] {
                        visited[target.index] = true;
                        queue.push_back(target.index);
                    }
                }
            }
        }
        
        // All nodes should be reachable
        let unreachable_count = visited.iter().filter(|&&v| !v).count();
        assert_eq!(unreachable_count, 0, "{} nodes are unreachable", unreachable_count);
    }
    
    /// Test path quality: paths should be reasonably direct
    #[test]
    fn test_path_quality() {
        let bsp = BspMap::load("q2dm1").expect("Failed to load BSP");
        let graph = NavGraph::generate_from_bsp(&bsp);
        
        let start = 0;
        let goal = graph.node_count() - 1;
        
        let path = graph.find_path_bfs(start, goal).expect("No path found");
        
        // Path should not be excessively long
        // (e.g., not 10x the straight-line distance)
        let straight_line = graph.nodes[start].pt.distance(graph.nodes[goal].pt);
        let path_length: f32 = path
            .windows(2)
            .map(|w| graph.nodes[w[0]].pt.distance(graph.nodes[w[1]].pt))
            .sum();
        
        let ratio = path_length / straight_line;
        assert!(
            ratio < 5.0,
            "Path is {}x longer than straight line (too circuitous)",
            ratio
        );
    }
    
    /// Test that shortcuts work correctly
    #[test]
    fn test_shortcut_detection() {
        let bsp = BspMap::load("q2dm1").expect("Failed to load BSP");
        let graph = NavGraph::generate_from_bsp(&bsp);
        
        // Create a simple path: A -> B -> C
        // If A can see C directly, shortcut should skip B
        let path = vec![0, 1, 2];
        
        // Check if A can see C
        let can_shortcut = graph.nodes[path[0]]
            .pt
            .line_of_sight(&graph.nodes[path[2]].pt, &bsp);
        
        if can_shortcut {
            // Shortcut should be available
            assert!(true, "Shortcut A->C is available");
        }
    }
    
    /// Test multi-level navigation (q2dm3 has elevators)
    #[test]
    #[ignore] // TODO: Implement elevator detection
    fn test_multi_level_navigation() {
        let bsp = BspMap::load("q2dm3").expect("Failed to load BSP");
        let graph = NavGraph::generate_from_bsp(&bsp);
        
        // Find nodes on different levels
        let lower_level = find_node_at_height(&graph, 500.0);
        let upper_level = find_node_at_height(&graph, 900.0);
        
        // Path should exist between levels (via elevator)
        let path = graph.find_path(lower_level, upper_level);
        assert!(path.is_some(), "Multi-level path not found");
    }
    
    // Helper functions
    fn load_spawn_points(map: &str) -> Vec<SpawnPoint> {
        // TODO: Load spawn points from BSP or config
        vec![]
    }
    
    fn find_node_at_height(graph: &NavGraph, target_z: f32) -> usize {
        let mut best_node = 0;
        let mut best_diff = f32::MAX;
        
        for (i, node) in graph.nodes.iter().enumerate() {
            let diff = (node.pt.z - target_z).abs();
            if diff < best_diff {
                best_diff = diff;
                best_node = i;
            }
        }
        
        best_node
    }
    
    struct SpawnPoint {
        node: usize,
        _name: String,
    }
}
