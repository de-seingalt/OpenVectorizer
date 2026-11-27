import { expect, test } from '@playwright/test';

test('example image is vectorized and can be compared', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));

  await page.goto('/');
  await expect(page.getByText('Déposez une image ici')).toBeVisible();
  await page.screenshot({ path: 'test-results/screens/01-empty.png' });

  await page.getByRole('button', { name: 'Essayer un exemple' }).click();
  await expect(page.locator('footer')).toContainText('formes', { timeout: 15_000 });
  await expect(page.getByRole('img', { name: 'Vecteur' })).toBeVisible();
  await page.screenshot({ path: 'test-results/screens/02-result.png' });

  // The palette is shown, and limiting the number of colors re-vectorizes with that many.
  const swatches = page.getByRole('list', { name: /couleurs dans le résultat/ }).getByRole('listitem');
  await expect(swatches).toHaveCount(4); // background, dark blue, yellow, orange: no blend color
  await page.getByRole('switch', { name: 'Automatique' }).click();
  const slider = page.getByRole('slider', { name: 'Nombre maximal de couleurs du résultat' });
  await slider.focus();
  for (let i = 0; i < 40; i++) await slider.press('ArrowLeft');
  await expect(swatches).toHaveCount(2);
  await page.screenshot({ path: 'test-results/screens/02b-two-colors.png' });

  // Changing an option triggers a new vectorization.
  await page.getByRole('radio', { name: 'Photo' }).click();
  await expect(page.locator('footer')).toContainText('formes');
  await page.getByRole('radio', { name: 'Vecteur' }).click();
  await page.screenshot({ path: 'test-results/screens/03-vector-only.png' });

  expect(errors).toEqual([]);
});
