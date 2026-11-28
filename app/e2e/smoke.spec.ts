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

  // Palette editor: one large bubble per output color, small bubbles = tints.
  const colors = page.getByRole('button', { name: /^Couleur #/ });
  await expect(colors).toHaveCount(4); // background, dark blue, yellow, orange: no blend color
  await page.screenshot({ path: 'test-results/screens/02a-palette.png' });

  // Drag the orange color onto the yellow one: they merge.
  const orange = colors.filter({ has: page.getByRole('button', { name: /^Teinte #e6/ }) });
  const yellow = colors.filter({ has: page.getByRole('button', { name: /^Teinte #e9/ }) });
  await orange.dragTo(yellow);
  await expect(colors).toHaveCount(3);

  // Keyboard split: pick the orange tint, then "Nouvelle couleur".
  await page.getByRole('button', { name: /^Teinte #e6/ }).press('Enter');
  await page.getByRole('button', { name: 'Nouvelle couleur' }).click();
  await expect(colors).toHaveCount(4);
  await page.getByRole('button', { name: 'Annuler les modifications de palette' }).click();

  // Limiting the number of colors regroups the tints.
  await page.getByRole('switch', { name: 'Automatique' }).click();
  const slider = page.getByRole('slider', { name: 'Nombre maximal de couleurs du résultat' });
  await slider.focus();
  for (let i = 0; i < 40; i++) await slider.press('ArrowLeft');
  await expect(colors).toHaveCount(2);
  await page.screenshot({ path: 'test-results/screens/02b-two-colors.png' });

  // Zoom: the buttons and the wheel change the zoom level (and nothing resets it).
  const zoomLevel = page.locator('[role=toolbar] span.tabular-nums');
  const level = async () => Number((await zoomLevel.textContent())?.replace(/\D/g, ''));
  const before = await level();
  await page.getByRole('button', { name: 'Zoom avant' }).click();
  await expect.poll(level).toBeGreaterThan(before);
  await page.mouse.move(400, 400);
  await page.mouse.wheel(0, 400);
  await expect.poll(level).toBeLessThan(before);
  await page.getByRole('button', { name: 'Ajuster' }).click();
  await expect.poll(level).toBe(before);

  // Corners tool: all round → no sharp corner; all sharp → every corner is sharp.
  const cornerCount = page.locator('#corners-label');
  await page.getByRole('radio', { name: 'Tous ronds' }).click();
  await expect(cornerCount).toContainText(/\b0\s+vifs\s+sur\s+[1-9]/);
  await page.getByRole('radio', { name: 'Tous vifs' }).click();
  await expect(cornerCount).toHaveText(/(\d+)\s+vifs\s+sur\s+\1\s*$/);

  // Per-corner editing: double-click a marker rounds that corner (and its group).
  await page.getByRole('switch', { name: 'Afficher les coins' }).click();
  const marker = page.locator('[data-corner]').first();
  // (A corner is drawn as an L: its bounding box center is off the stroke, so dispatch.)
  await marker.dispatchEvent('dblclick');
  await expect(cornerCount).not.toHaveText(/(\d+)\s+vifs\s+sur\s+\1\s*$/);
  const resetCorners = page.getByRole('button', { name: /Annuler les retouches de coins/ });
  await expect(resetCorners).toBeVisible();
  // A click selects it: the panel shows it rounded, with its radius.
  await page.locator('[data-corner][aria-label^="Arrondi"]').first().dispatchEvent('click');
  const panel = page.locator('[aria-labelledby="corner-title"]');
  await expect(panel.getByRole('radio', { name: 'Arrondi' })).toBeChecked();
  await expect(panel.getByRole('slider', { name: 'Rayon' })).toBeVisible();
  await page.screenshot({ path: 'test-results/screens/02d-corner-edit.png' });
  // Dragging the arc handle toward the apex makes the corner sharp.
  const handle = page.locator('[data-corner-handle]');
  const hb = await handle.boundingBox();
  if (!hb) throw new Error('no radius handle');
  await page.mouse.move(hb.x + hb.width / 2, hb.y + hb.height / 2);
  await page.mouse.down();
  for (let k = 1; k <= 5; k++) {
    // Toward and past the apex (bottom right).
    await page.mouse.move(hb.x + hb.width / 2 + 12 * k, hb.y + hb.height / 2 + 12 * k);
  }
  await page.mouse.up();
  await expect(panel.getByRole('radio', { name: 'Vif' })).toBeChecked();
  await panel.getByRole('radio', { name: 'Vif' }).click();
  await resetCorners.click();
  await expect(cornerCount).toHaveText(/(\d+)\s+vifs\s+sur\s+\1\s*$/);
  await page.keyboard.press('Escape');
  await expect(panel).toBeHidden();
  const autoCorners = page.getByRole('radio', { name: 'Auto', exact: true });
  await autoCorners.click();
  await expect(autoCorners).toBeChecked();
  await page.screenshot({ path: 'test-results/screens/02c-corners.png' });

  // Guides: show them, turn one line off (the tracing ignores it), then undo the choice.
  await page.getByRole('switch', { name: /Afficher les guides/ }).click();
  const guidesCount = page.locator('#guides-count');
  await expect(guidesCount).toContainText(/[1-9]\d*\s+suivis/);
  // (A horizontal line has an empty bounding box: dispatch the click instead of aiming at it.)
  await page.locator('[data-guide][data-status="followed"][aria-label^="Droite"]').first().dispatchEvent('click');
  await expect(page.locator('[data-guide][data-status="off"]')).toHaveCount(1);
  await page.screenshot({ path: 'test-results/screens/02e-guides.png' });
  await page.getByRole('button', { name: /Annuler les choix de guides/ }).click();
  await expect(page.locator('[data-guide][data-status="off"]')).toHaveCount(0);
  // Geometry 0: shapes are only candidates; a click forces one, which the tracing then follows.
  const geometry = page.getByRole('slider', { name: 'Géométrie' });
  await geometry.focus();
  await geometry.press('Home');
  await expect(guidesCount).toContainText(/[1-9]\d*\s+candidats/);
  await page.locator('[data-guide][data-status="candidate"]').first().dispatchEvent('click');
  await expect(page.locator('[data-guide][data-status="forced"]')).toHaveCount(1);
  await page.screenshot({ path: 'test-results/screens/02f-forced-guide.png' });
  await page.getByRole('button', { name: /Annuler les choix de guides/ }).click();
  await geometry.press('End');

  // Changing an option triggers a new vectorization.
  await page.getByRole('radio', { name: 'Photo' }).click();
  await expect(page.locator('footer')).toContainText('formes');
  await page.getByRole('radio', { name: 'Vecteur' }).click();
  await page.screenshot({ path: 'test-results/screens/03-vector-only.png' });

  expect(errors).toEqual([]);
});
