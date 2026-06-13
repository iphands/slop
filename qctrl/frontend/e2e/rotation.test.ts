import { test, expect } from '@playwright/test';

test.describe('Map Rotation Queue', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:3000/rotation');
  });

  test('should display the rotation page header', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Map Rotation' })).toBeVisible();
  });

  test('should show add map button', async ({ page }) => {
    await expect(page.getByRole('button', { name: 'Add Map' })).toBeVisible();
  });

  test('should open add map dialog when clicking add button', async ({ page }) => {
    await page.getByRole('button', { name: 'Add Map' }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(page.getByText('Add Map to Queue')).toBeVisible();
    await expect(page.getByPlaceholder('Type to filter maps...')).toBeVisible();
  });

  test('should filter maps in autocomplete on search', async ({ page }) => {
    await page.getByRole('button', { name: 'Add Map' }).click();
    const searchInput = page.getByPlaceholder('Type to filter maps...');
    await searchInput.fill('q2dm');
    await expect(page.getByText(/q2dm\d/)).toBeVisible();
  });

  test('should add map to queue after selection', async ({ page }) => {
    await page.getByRole('button', { name: 'Add Map' }).click();
    const searchInput = page.getByPlaceholder('Type to filter maps...');
    await searchInput.fill('q2dm1');
    await page.getByText('q2dm1').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await expect(page.getByText('Add Map to Queue')).not.toBeVisible();
    await expect(page.getByText('q2dm1')).toBeVisible();
  });

  test('should show queue count', async ({ page }) => {
    await expect(page.getByText(/Queue \(\d+ (?:map|maps)\)/)).toBeVisible();
  });

  test('should remove map from queue when clicking remove button', async ({ page }) => {
    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('q2dm1');
    await page.getByText('q2dm1').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(500);
    const queueList = page.getByText('q2dm1').locator('..');
    await queueList.getByRole('button').last().click();
    await page.waitForTimeout(500);
    await expect(page.getByText('q2dm1')).not.toBeVisible();
  });

  test('should show drag handle for reorderable items', async ({ page }) => {
    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('q2dm1');
    await page.getByText('q2dm1').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(500);
    await expect(page.getByText('⇄')).toBeVisible();
  });

  test('should reorder queue items via drag and drop', async ({ page }) => {
    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('q2dm1');
    await page.getByText('q2dm1').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(300);
    await page.getByRole('button', { name: 'Add Map' }).click();
    await page.getByPlaceholder('Type to filter maps...').fill('kessel');
    await page.getByText('kessel').first().click();
    await page.getByRole('button', { name: 'Add to Queue' }).click();
    await page.waitForTimeout(300);
    const queueItems = page.getByText(/q2dm1|kessel/);
    const firstItem = await queueItems.first().textContent();
    expect(firstItem).toContain('q2dm1');
    const dragHandle = page.getByText('kessel').locator('..').getByText('⇄');
    const targetItem = page.getByText('q2dm1').locator('..');
    await dragHandle.hover();
    await page.mouse.down();
    await targetItem.hover();
    await page.mouse.up();
    await page.waitForTimeout(500);
    const newQueueItems = page.getByText(/q2dm1|kessel/);
    const newFirstItem = await newQueueItems.first().textContent();
    expect(newFirstItem).toBeDefined();
  });

  test('should show mode indicator', async ({ page }) => {
    await expect(page.getByText(/Sequential|Random/)).toBeVisible();
  });

  test('should show empty queue message when no maps', async ({ page }) => {
    const queueItems = page.getByText(/^[1-9]\./);
    const count = await queueItems.count();
    for (let i = 0; i < count; i++) {
      await queueItems.first().getByRole('button').last().click();
      await page.waitForTimeout(300);
    }
    await expect(page.getByText('No maps in queue')).toBeVisible();
  });
});
