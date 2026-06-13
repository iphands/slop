import { test, expect } from '@playwright/test';

const API_BASE = 'http://localhost:3000/api';

async function clearRotationQueue(page: test.Page) {
  const response = await page.request.get(`${API_BASE}/rotation`);
  const data = await response.json();
  const maps = (data as { maps: string[] }).maps || [];
  
  for (const mapName of maps) {
    await page.request.delete(`${API_BASE}/rotation/${encodeURIComponent(mapName)}`);
  }
}

async function addMapsToQueue(page: test.Page, mapNames: string[]) {
  for (const mapName of mapNames) {
    await page.request.post(`${API_BASE}/rotation`, {
      data: { map: mapName },
    });
  }
}

async function getRotationQueue(page: test.Page) {
  const response = await page.request.get(`${API_BASE}/rotation`);
  return response.json();
}

test.describe('Map Rotation - Full Integration', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:3000/rotation');
    await clearRotationQueue(page);
  });

  test.afterEach(async ({ page }) => {
    await clearRotationQueue(page);
  });

  test('adds 3 maps to queue and displays in order', async ({ page }) => {
    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('q2dm1');
    await page.getByText('q2dm1').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(500);

    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('kessel');
    await page.getByText('kessel').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(500);

    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('dm6');
    await page.getByText('dm6').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(500);

    await expect(page.getByText(/Queue \(3 maps\)/)).toBeVisible();
    await expect(page.getByText('1. q2dm1')).toBeVisible();
    await expect(page.getByText('2. kessel')).toBeVisible();
    await expect(page.getByText('3. dm6')).toBeVisible();
  });

  test('Sequential mode: reorders via drag-drop and handles empty queue', async ({ page }) => {
    await addMapsToQueue(page, ['q2dm1', 'kessel', 'dm6']);
    await page.waitForTimeout(500);

    await expect(page.getByText('Sequential')).toBeVisible();

    const queue = await getRotationQueue(page);
    expect(queue.maps).toEqual(['q2dm1', 'kessel', 'dm6']);
    expect(queue.mode).toBe('Sequential');

    const q2dm1Item = page.getByText('q2dm1').first();
    const kesselItem = page.getByText('kessel').first();
    const dragHandle = kesselItem.locator('..').getByText('⇄');
    const target = q2dm1Item.locator('..');
    
    await dragHandle.hover();
    await page.mouse.down();
    await target.hover();
    await page.mouse.up();
    await page.waitForTimeout(500);

    const reorderedQueue = await getRotationQueue(page);
    expect(reorderedQueue.maps[0]).toBe('kessel');
    expect(reorderedQueue.maps[1]).toBe('q2dm1');
    expect(reorderedQueue.maps[2]).toBe('dm6');

    await page.getByText('kessel').first().locator('..').getByRole('button').last().click();
    await page.waitForTimeout(300);
    await page.getByText('q2dm1').first().locator('..').getByRole('button').last().click();
    await page.waitForTimeout(300);
    await page.getByText('dm6').first().locator('..').getByRole('button').last().click();
    await page.waitForTimeout(500);

    await expect(page.getByText('No maps in queue')).toBeVisible();
  });

  test('Random mode: switches from Sequential', async ({ page }) => {
    await addMapsToQueue(page, ['q2dm1', 'kessel', 'dm6']);
    await page.waitForTimeout(500);

    await page.getByRole('button', { name: /Random/ }).click();
    await page.waitForTimeout(500);

    await expect(page.getByText('Random')).toBeVisible();
    
    const queue = await getRotationQueue(page);
    expect(queue.mode).toBe('Random');
  });

  test('Empty queue in Random mode', async ({ page }) => {
    await clearRotationQueue(page);
    await page.waitForTimeout(500);

    await expect(page.getByText('No maps in queue')).toBeVisible();

    await page.getByRole('button', { name: /Random/ }).click();
    await page.waitForTimeout(500);

    const queue = await getRotationQueue(page);
    expect(queue.maps).toEqual([]);
    expect(queue.mode).toBe('Random');
  });

  test('Conflict resolution: rapid mode switching persists final state', async ({ page }) => {
    await addMapsToQueue(page, ['q2dm1', 'kessel']);
    await page.waitForTimeout(500);

    const initialQueue = await getRotationQueue(page);
    expect(initialQueue.maps.length).toBe(2);

    await page.getByRole('button', { name: /Sequential/ }).click();
    await page.waitForTimeout(200);
    await page.getByRole('button', { name: /Random/ }).click();
    await page.waitForTimeout(200);
    await page.getByRole('button', { name: /Sequential/ }).click();
    await page.waitForTimeout(500);

    const finalQueue = await getRotationQueue(page);
    expect(finalQueue.mode).toBe('Sequential');
    expect(finalQueue.maps.length).toBe(2);
  });

  test('removes map from queue', async ({ page }) => {
    await addMapsToQueue(page, ['q2dm1', 'kessel', 'dm6']);
    await page.waitForTimeout(500);

    const kesselItem = page.getByText('kessel').first().locator('..');
    await kesselItem.getByRole('button').last().click();
    await page.waitForTimeout(500);

    await expect(page.getByText('1. q2dm1')).toBeVisible();
    await expect(page.getByText('2. dm6')).toBeVisible();
    await expect(page.getByText('kessel')).not.toBeVisible();
    await expect(page.getByText(/Queue \(2 maps\)/)).toBeVisible();

    const queue = await getRotationQueue(page);
    expect(queue.maps).toEqual(['q2dm1', 'dm6']);
  });

  test('reorders queue via drag and drop', async ({ page }) => {
    await addMapsToQueue(page, ['q2dm1', 'kessel', 'dm6']);
    await page.waitForTimeout(500);

    const dm6Item = page.getByText('dm6').first();
    const q2dm1Item = page.getByText('q2dm1').first();
    const dm6Handle = dm6Item.locator('..').getByText('⇄');
    const target = q2dm1Item.locator('..');
    
    await dm6Handle.hover();
    await page.mouse.down();
    await target.hover();
    await page.mouse.up();
    await page.waitForTimeout(500);

    const queue = await getRotationQueue(page);
    expect(queue.maps[0]).toBe('dm6');
    expect(queue.maps[1]).toBe('q2dm1');
    expect(queue.maps[2]).toBe('kessel');
  });

  test('Mode toggle switches between Sequential and Random', async ({ page }) => {
    await addMapsToQueue(page, ['q2dm1']);
    await page.waitForTimeout(500);

    await expect(page.getByText('Sequential')).toBeVisible();

    await page.getByRole('button', { name: /Random/ }).click();
    await page.waitForTimeout(500);

    await expect(page.getByText('Random')).toBeVisible();

    const randomQueue = await getRotationQueue(page);
    expect(randomQueue.mode).toBe('Random');

    await page.getByRole('button', { name: /Sequential/ }).click();
    await page.waitForTimeout(500);

    await expect(page.getByText('Sequential')).toBeVisible();

    const seqQueue = await getRotationQueue(page);
    expect(seqQueue.mode).toBe('Sequential');
  });

  test('Queue persists across page refresh', async ({ page }) => {
    await addMapsToQueue(page, ['q2dm1', 'kessel']);
    await page.waitForTimeout(500);

    await page.reload();
    await page.waitForTimeout(500);

    await expect(page.getByText(/Queue \(2 maps\)/)).toBeVisible();
    await expect(page.getByText('1. q2dm1')).toBeVisible();
    await expect(page.getByText('2. kessel')).toBeVisible();

    const queue = await getRotationQueue(page);
    expect(queue.maps).toEqual(['q2dm1', 'kessel']);
  });

  test('prevents duplicate map in queue', async ({ page }) => {
    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('q2dm1');
    await page.getByText('q2dm1').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(500);

    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('q2dm1');
    await page.getByText('q2dm1').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(500);

    const queue = await getRotationQueue(page);
    const duplicateCount = queue.maps.filter(m => m === 'q2dm1').length;
    expect(duplicateCount).toBe(1);
  });

  test('Autocomplete filters maps correctly', async ({ page }) => {
    await page.getByRole('button', { name: 'Add Map' }).click();

    await page.getByPlaceholder('Type to filter maps...').fill('q2dm');
    await expect(page.getByText(/q2dm\d/).first()).toBeVisible();

    await page.getByPlaceholder('Type to filter maps...').fill('zzznonexistent');
    await expect(page.getByText('No maps match your search')).toBeVisible();
  });
});

test.describe('Map Rotation - Cycle Simulation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:3000/rotation');
  });

  test('Sequential mode: simulates rotation cycle', async ({ page }) => {
    await addMapsToQueue(page, ['q2dm1', 'kessel', 'dm6']);
    await page.waitForTimeout(500);

    let queue = await getRotationQueue(page);
    expect(queue.maps).toEqual(['q2dm1', 'kessel', 'dm6']);
    expect(queue.mode).toBe('Sequential');

    await page.getByText('q2dm1').first().locator('..').getByRole('button').last().click();
    await page.waitForTimeout(500);

    queue = await getRotationQueue(page);
    expect(queue.maps).toEqual(['kessel', 'dm6']);

    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('q2dm1');
    await page.getByText('q2dm1').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(500);

    queue = await getRotationQueue(page);
    expect(queue.maps).toEqual(['kessel', 'dm6', 'q2dm1']);
  });

  test('Random mode: verifies mode configuration', async ({ page }) => {
    await addMapsToQueue(page, ['q2dm1', 'kessel', 'dm6']);
    await page.waitForTimeout(500);

    await page.getByRole('button', { name: /Random/ }).click();
    await page.waitForTimeout(500);

    const queue = await getRotationQueue(page);
    expect(queue.mode).toBe('Random');
    expect(queue.maps).toContain('q2dm1');
    expect(queue.maps).toContain('kessel');
    expect(queue.maps).toContain('dm6');
  });
});
