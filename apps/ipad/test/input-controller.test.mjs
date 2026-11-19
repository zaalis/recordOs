import test from 'node:test';
import assert from 'node:assert/strict';
import { InputController } from '../src/stage/input-controller.ts';
import { DEFAULT_PREFERENCES } from '../src/lib/storage.ts';

globalThis.window = globalThis;
globalThis.requestAnimationFrame = fn => setTimeout(fn, 16);
globalThis.cancelAnimationFrame = id => clearTimeout(id);
function setup(settings = {}, mode = 'pencil') {
 const events = [], actions = [], surface = new EventTarget(); surface.setPointerCapture = () => {};
 const controller = new InputController(surface, () => ({left:0,top:0,width:1000,height:1000}), {reliable:b=>events.push(b),realtime:b=>events.push(b)}, {...DEFAULT_PREFERENCES, ...settings}, {onToggleInterface(){},onFeedback(){},onUndo(){},onRedo(){},onActivity(){},modifiersForClick(){},onInputSource(){},onCreativeTap(action){actions.push(action)}}, 'blender');
 controller.mode=mode; const detach=controller.attach();
 const emit=(type,pointerType,pointerId,x,y,buttons=0,pressure=.5)=>{const e=new Event(type,{cancelable:true});Object.assign(e,{pointerType,pointerId,clientX:x,clientY:y,buttons,pressure,tiltX:0,tiltY:0});surface.dispatchEvent(e);};
 return {controller,events,actions,emit,detach};
}
const kind=b=>new Uint8Array(b)[0];
test('finger navigation excludes Pencil hover/contact until the last finger lifts',()=>{
 const {events,emit,detach}=setup();
 emit('pointermove','pen',9,500,500); // hover must not block genuine finger navigation
 emit('pointerdown','touch',1,100,100);emit('pointerdown','touch',2,200,100);
 emit('pointermove','touch',1,112,100);emit('pointermove','touch',2,212,100);emit('pointermove','touch',1,122,100);emit('pointermove','touch',2,222,100);
 const before=events.length;emit('pointermove','pen',9,800,800);emit('pointerdown','pen',9,800,800,1);
 assert.equal(events.length,before,'Pencil cannot drive the cursor during finger navigation');
 assert.ok(events.some(b=>kind(b)===2),'Blender uses relative movement');
 assert.equal(events.some(b=>kind(b)===1),false,'navigation never warps to an absolute anchor');
 emit('pointerup','touch',1,140,100);emit('pointerup','touch',2,240,100);
 const after=events.length;emit('pointerdown','pen',9,600,600,1);assert.ok(events.length>after,'Pencil resumes after the finger gesture');detach();
});
test('Pencil contact owns input; finger attempts do not alter the stroke',()=>{
 const {events,emit,detach}=setup();emit('pointerdown','pen',9,500,500,1);const before=events.length;
 emit('pointerdown','touch',1,100,100);emit('pointermove','touch',1,400,400);assert.equal(events.length,before);
 emit('pointermove','pen',9,550,550,1);assert.ok(events.length>before);emit('pointerup','pen',9,550,550);detach();
});
test('cancelled navigation releases its middle button and Shift modifier',()=>{
 const {events,emit,detach}=setup();emit('pointerdown','touch',1,100,100);emit('pointerdown','touch',2,200,100);emit('pointermove','touch',1,112,100);emit('pointermove','touch',2,212,100);
 emit('pointercancel','touch',1,140,100);
 assert.ok(events.some(b=>kind(b)===3&&new Uint8Array(b)[4]===2&&new Uint8Array(b)[5]===0));
 assert.ok(events.some(b=>kind(b)===5&&new Uint8Array(b)[5]===0));detach();
});

const touch = (emit, type, id, x = 180, y = 240) => emit(type, 'touch', id, x, y);

test('creative double tap changes tools once and suppresses the single tap', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const { emit, events, actions, detach } = setup({ creativeTouch: true }, 'direct');
  touch(emit, 'pointerdown', 1);
  touch(emit, 'pointerup', 1);
  t.mock.timers.tick(100);
  touch(emit, 'pointerdown', 2, 188);
  touch(emit, 'pointerup', 2, 188);
  t.mock.timers.tick(300);
  assert.deepEqual(actions, ['swap-tool']);
  assert.equal(events.length, 0);
  detach();
});

test('a single creative tap opens the palette and cancellation drops a pending tap', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const { controller, emit, actions, detach } = setup({ creativeTouch: true }, 'direct');
  touch(emit, 'pointerdown', 1);
  touch(emit, 'pointerup', 1);
  t.mock.timers.tick(270);
  assert.deepEqual(actions, ['palette']);
  touch(emit, 'pointerdown', 2);
  touch(emit, 'pointerup', 2);
  controller.releaseAll();
  t.mock.timers.tick(300);
  assert.deepEqual(actions, ['palette']);
  detach();
});

test('direct touch retains immediate Windows clicks unless creative gestures are enabled', () => {
  const { emit, events, actions, detach } = setup({}, 'direct');
  touch(emit, 'pointerdown', 1);
  touch(emit, 'pointerup', 1);
  assert.deepEqual(events.map(kind), [3, 3]);
  assert.deepEqual(actions, []);
  detach();
});

test('stylus pressure keeps a light initial contact and respects sensitivity', () => {
  const { emit, events, detach } = setup({ pencilSensitivity: 1.5 });
  emit('pointerdown', 'pen', 9, 500, 500, 1, 0);
  emit('pointermove', 'pen', 9, 520, 520, 1, 0.4);
  emit('pointermove', 'pen', 9, 540, 540, 1, 0.9);
  const pressure = events.filter(b => kind(b) === 7).map(b => new DataView(b).getFloat32(16, true));
  assert.equal(pressure[0], 0);
  assert.ok(Math.abs(pressure[1] - 0.6) < 1e-6);
  assert.equal(pressure[2], 1);
  detach();
});
