import assert from 'node:assert/strict';
import test from 'node:test';
import { DEFAULT_PREFERENCES, preferencesFrom } from '../src/lib/storage.ts';

test('a fresh install gets the defaults', () => {
  assert.deepEqual(preferencesFrom(undefined), DEFAULT_PREFERENCES);
});

test('screen-streaming preferences of earlier versions are dropped, the rest is kept', () => {
  const prefs = preferencesFrom({
    deviceName: 'Studio iPad',
    controlMode: 'pencil',
    stageMode: 'stream',
    video: { profile: 'balanced', resolution: '1080p', fps: 60, bitrate: 'auto', codec: 'h264' },
    videoDefaultVersion: 4,
    pinchAction: 'zoom-view',
    showLocalCursor: true,
    language: 'en',
    toolbar: { edge: 'left' },
  });
  for (const key of ['stageMode', 'video', 'videoDefaultVersion', 'pinchAction', 'showLocalCursor', 'language']) {
    assert.equal(key in prefs, false, key);
  }
  assert.equal(prefs.deviceName, 'Studio iPad');
  assert.equal(prefs.controlMode, 'pencil');
  assert.deepEqual(prefs.toolbar, { ...DEFAULT_PREFERENCES.toolbar, edge: 'left' });
  assert.equal(prefs.autoHideToolbar, true);
});


test('surface style keeps defaults on upgrade and bounds stored customization',()=>{
 const value=preferencesFrom({surfaceStyle:{trail:true,trailColor:'not-a-color',trailDuration:99999,patternSpacing:-1,pattern:'bad'}}).surfaceStyle;
 assert.equal(value.trail,true);assert.equal(value.trailColor,DEFAULT_PREFERENCES.surfaceStyle.trailColor);assert.equal(value.trailDuration,1500);assert.equal(value.patternSpacing,8);assert.equal(value.pattern,'dots');
});
