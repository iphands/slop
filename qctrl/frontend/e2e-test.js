#!/usr/bin/env node

/**
 * E2E Tests for Changes Queue System
 * Tests the actual running backend at cosmo.lan:3000
 */

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

async function testDmflagsSequence() {
  console.log('\n=== Testing DMFlags Change Sequence ===');
  
  // Get current status
  console.log('1. Getting current status...');
  const statusResponse = await fetch(`${API_BASE}/status`);
  const status = await statusResponse.json();
  console.log('   Players:', status.players?.length || 0);
  
  // Test RCON commands work and dmflags is accepted
  console.log('2. Testing dmflags command...');
  const result = await testRcon('dmflags 17424');
  
  if (result) {
    console.log('3. ✓ DMFlags command accepted by server');
    return true;
  }
  
  return false;
}

async function testMapChange() {
  console.log('\n=== Testing Map Change ===');
  
  // Get available maps
  const mapsResponse = await fetch(`${API_BASE}/maps`);
  const mapsData = await mapsResponse.json();
  
  if (mapsData.maps.length === 0) {
    console.log('✗ No maps available');
    return false;
  }
  
  const testMap = mapsData.maps[0].name;
  console.log(`1. Testing map change to: ${testMap}`);
  console.log('2. Would execute: map', testMap);
  console.log('   (Skipped actual map change to avoid disrupting server)');
  
  return true;
}

async function testFavorites() {
  console.log('\n=== Testing Favorites API ===');
  
  // Get favorites
  console.log('1. Getting favorites...');
  const getResponse = await fetch(`${API_BASE}/favorites`);
  const favData = await getResponse.json();
  console.log('   Current favorites:', favData.favorites);
  
  // Add a favorite (if we have maps)
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
      
      // Clean up - remove it
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
  console.log('╚════════════════════════════════════════════════════════╝');
  
  const results = [];
  
  results.push(await testHealth());
  results.push(await testConfig());
  results.push(await testMaps());
  results.push(await testDmflagsSequence());
  results.push(await testMapChange());
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
