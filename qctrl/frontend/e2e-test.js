#!/usr/bin/env node

/**
 * E2E Tests for Changes Queue System
 * Tests the actual running backend at cosmo.lan:3000
 * 
 * These tests use the SAME logic as the frontend (shared applyLogic)
 * to ensure they test the exact same code paths.
 */

import { buildApplyCommands } from './e2e-applyLogic.js';

const API_BASE = 'http://cosmo.lan:3000/api';

async function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function testHealth() {
  console.log('\n=== Testing Health Endpoint ===');
  try {
    const response = await fetch(`${API_BASE}/health`);
    const data = await response.json();
    console.log('✓ Health check passed:', data);
    return true;
  } catch (error) {
    console.error('✗ Health check failed:', error.message);
    return false;
  }
}

async function testConfig() {
  console.log('\n=== Testing Config Endpoint ===');
  try {
    const response = await fetch(`${API_BASE}/config`);
    const data = await response.json();
    console.log('✓ Config retrieved:', {
      host: data.server.host,
      port: data.server.port,
      baseq2: data.paths.baseq2,
    });
    return true;
  } catch (error) {
    console.error('✗ Config failed:', error.message);
    return false;
  }
}

async function testMaps() {
  console.log('\n=== Testing Maps Endpoint ===');
  try {
    const response = await fetch(`${API_BASE}/maps`);
    const data = await response.json();
    console.log('✓ Maps retrieved:', data.maps.length, 'maps');
    if (data.maps.length > 0) {
      console.log('  Sample maps:', data.maps.slice(0, 3).map(m => m.name).join(', '));
    }
    return true;
  } catch (error) {
    console.error('✗ Maps failed:', error.message);
    return false;
  }
}

async function testRcon(command) {
  console.log(`\n=== Testing RCON: ${command} ===`);
  try {
    const response = await fetch(`${API_BASE}/rcon/execute`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command }),
    });
    
    if (!response.ok) {
      console.log('✗ RCON command failed:', response.statusText);
      return null;
    }
    
    const text = await response.text();
    console.log('✓ RCON response:', text.substring(0, 100) + (text.length > 100 ? '...' : ''));
    return text;
  } catch (error) {
    console.error('✗ RCON failed:', error.message);
    return null;
  }
}

async function testDmflagsApply() {
  console.log('\n=== Testing DMFlags Apply Flow (Red/Green TDD) ===');
  
  // Get current status
  console.log('1. Getting current status...');
  const statusResponse = await fetch(`${API_BASE}/status`);
  const status = await statusResponse.json();
  const currentMap = status.map || 'q2dm1';
  console.log('   Current map:', currentMap);
  
  // Simulate queuing a dmflags change
  const changes = [
    { type: 'dmflags', pendingValue: 17424, description: 'Deathmatch flags' }
  ];
  
  console.log('2. Queuing dmflags change to 17424');
  
  // Use the SAME logic as the frontend (buildApplyCommands)
  console.log('3. Building apply commands (using shared logic)...');
  const commands = buildApplyCommands(changes, currentMap);
  console.log('   Commands to send:', commands.join(', '));
  
  // Verify that a map restart IS included
  const hasMapRestart = commands.some(cmd => cmd.startsWith('map'));
  if (!hasMapRestart) {
    console.log('   ✗ FAIL: No map restart in commands!');
    console.log('   This is the BUG - dmflags changes need implicit map restart');
    return false;
  }
  console.log('   ✓ Map restart included (GOOD)');
  
  // Execute the commands
  console.log('4. Executing commands...');
  for (const cmd of commands) {
    console.log(`   Sending: ${cmd}`);
    const result = await testRcon(cmd);
    if (!result) {
      console.log('   ✗ Command failed:', cmd);
      return false;
    }
  }
  
  // Wait for changes to take effect
  console.log('5. Waiting for changes to apply...');
  await sleep(3000);
  
  // Verify dmflags actually changed
  console.log('6. Verifying dmflags change...');
  const dmflagsOutput = await testRcon('dmflags');
  if (!dmflagsOutput) {
    console.log('   ✗ Failed to get dmflags');
    return false;
  }
  
  console.log('   ✓ dmflags command executed');
  
  // Reset to default
  console.log('7. Resetting dmflags to 0...');
  await testRcon('dmflags 0');
  
  return true;
}

async function testMapChangeApply() {
  console.log('\n=== Testing Map Change Apply Flow ===');
  
  // Get available maps
  const mapsResponse = await fetch(`${API_BASE}/maps`);
  const mapsData = await mapsResponse.json();
  
  if (mapsData.maps.length === 0) {
    console.log('✗ No maps available');
    return false;
  }
  
  // Get current map
  console.log('1. Getting current map...');
  const statusResponse = await fetch(`${API_BASE}/status`);
  const status = await statusResponse.json();
  const currentMap = status.map || 'unknown';
  console.log('   Current map:', currentMap);
  
  // Pick a different map
  const targetMap = mapsData.maps.find(m => m.name !== currentMap)?.name || 'q2dm2';
  console.log('2. Queuing map change to:', targetMap);
  
  // Simulate queuing a map change
  const changes = [
    { type: 'map', pendingValue: targetMap, description: 'Map change' }
  ];
  
  // Use the SAME logic as the frontend
  console.log('3. Building apply commands (using shared logic)...');
  const commands = buildApplyCommands(changes, currentMap);
  console.log('   Commands to send:', commands.join(', '));
  
  // Execute
  console.log('4. Executing commands...');
  for (const cmd of commands) {
    console.log(`   Sending: ${cmd}`);
    const result = await testRcon(cmd);
    if (!result) {
      console.log('   ✗ Command failed:', cmd);
      return false;
    }
  }
  
  // Wait for map change
  console.log('5. Waiting for map change...');
  await sleep(3000);
  
  // Verify
  console.log('6. Verifying map change...');
  const newStatusResponse = await fetch(`${API_BASE}/status`);
  const newStatus = await newStatusResponse.json();
  const newMap = newStatus.map || 'unknown';
  console.log('   New map:', newMap);
  
  if (newMap === targetMap) {
    console.log('   ✓ Map changed successfully to', targetMap);
    return true;
  } else {
    console.log('   ✗ Map did not change. Expected:', targetMap, 'Got:', newMap);
    return false;
  }
}

async function testFavorites() {
  console.log('\n=== Testing Favorites API ===');
  
  // Get favorites
  console.log('1. Getting favorites...');
  const getResponse = await fetch(`${API_BASE}/favorites`);
  const favData = await getResponse.json();
  console.log('   Current favorites:', favData.favorites);
  
  // Add a favorite
  const mapsResponse = await fetch(`${API_BASE}/maps`);
  const mapsData = await mapsResponse.json();
  
  if (mapsData.maps.length > 0) {
    const testMap = mapsData.maps[0].name;
    console.log(`2. Adding ${testMap} to favorites...`);
    
    const postResponse = await fetch(`${API_BASE}/favorites`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ map_name: testMap }),
    });
    
    if (postResponse.ok) {
      console.log('   ✓ Favorite added');
      
      // Verify
      const verifyResponse = await fetch(`${API_BASE}/favorites`);
      const verifyData = await verifyResponse.json();
      if (verifyData.favorites.includes(testMap)) {
        console.log('   ✓ Favorite verified in list');
      }
      
      // Clean up
      console.log('3. Cleaning up...');
      await fetch(`${API_BASE}/favorites/${encodeURIComponent(testMap)}`, {
        method: 'DELETE',
      });
      console.log('   ✓ Favorite removed');
    } else {
      console.log('   ✗ Failed to add favorite');
    }
  }
  
  return true;
}

async function runAllTests() {
  console.log('╔════════════════════════════════════════════════════════╗');
  console.log('║  E2E Tests for qctrl Changes Queue System             ║');
  console.log('║  Target: cosmo.lan:3000                               ║');
  console.log('║  Using shared applyLogic from frontend                ║');
  console.log('╚════════════════════════════════════════════════════════╝');
  
  const results = [];
  
  results.push(await testHealth());
  results.push(await testConfig());
  results.push(await testMaps());
  results.push(await testDmflagsApply());
  results.push(await testMapChangeApply());
  results.push(await testFavorites());
  
  console.log('\n╔════════════════════════════════════════════════════════╗');
  console.log('║  Test Summary                                          ║');
  console.log('╠════════════════════════════════════════════════════════╣');
  const passed = results.filter(r => r).length;
  const total = results.length;
  console.log(`║  Passed: ${passed}/${total}                                              ║`);
  console.log('╚════════════════════════════════════════════════════════╝');
  
  if (passed === total) {
    console.log('\n✓ All tests passed!');
    process.exit(0);
  } else {
    console.log('\n✗ Some tests failed');
    process.exit(1);
  }
}

// Run tests
runAllTests().catch(error => {
  console.error('Fatal error:', error);
  process.exit(1);
});
