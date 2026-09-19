/**
 * Creative app profiles. A profile is pure data: labelled key combinations grouped in sections.
 * The agent only reports the foreground executable; the iPad maps it to a profile.
 * Users can duplicate a built-in profile and edit it, or create their own workspaces.
 */
import type { KeyName } from './keys.ts';

export type KeyMode = 'tap' | 'hold' | 'latch';

export interface KeyControl {
  type: 'key';
  id: string;
  label: string;
  keys: KeyName[];
  /** tap: press+release. hold: down while the finger stays. latch: toggles held state. */
  mode?: KeyMode;
  hint?: string;
  wide?: boolean;
}

export interface StepperControl {
  type: 'stepper';
  id: string;
  label: string;
  decrease: KeyName[];
  increase: KeyName[];
  hint?: string;
}

export type ProfileControl = KeyControl | StepperControl;

export interface ProfileSection {
  id: string;
  title: string;
  controls: ProfileControl[];
}

export interface AppProfile {
  id: string;
  name: string;
  /** Lower-case executable names that activate this profile automatically. */
  executables: string[];
  builtin: boolean;
  sections: ProfileSection[];
}

const key = (id: string, label: string, keys: KeyName[], extra: Partial<KeyControl> = {}): KeyControl => ({
  type: 'key',
  id,
  label,
  keys,
  ...extra,
});

const stepper = (id: string, label: string, decrease: KeyName[], increase: KeyName[], hint?: string): StepperControl => ({
  type: 'stepper',
  id,
  label,
  decrease,
  increase,
  ...(hint ? { hint } : {}),
});

export const GENERIC_PROFILE: AppProfile = {
  id: 'windows',
  name: 'Windows',
  executables: [],
  builtin: true,
  sections: [
    {
      id: 'edit',
      title: 'Edit',
      controls: [
        key('undo', 'Undo', ['Ctrl', 'Z']),
        key('redo', 'Redo', ['Ctrl', 'Y']),
        key('cut', 'Cut', ['Ctrl', 'X']),
        key('copy', 'Copy', ['Ctrl', 'C']),
        key('paste', 'Paste', ['Ctrl', 'V']),
        key('select-all', 'Select All', ['Ctrl', 'A']),
        key('save', 'Save', ['Ctrl', 'S']),
        key('find', 'Find', ['Ctrl', 'F']),
      ],
    },
    {
      id: 'windows',
      title: 'Windows',
      controls: [
        key('switch', 'Switch App', ['Alt', 'Tab']),
        key('task-view', 'Task View', ['Win', 'Tab']),
        key('desktop', 'Show Desktop', ['Win', 'D']),
        key('explorer', 'Explorer', ['Win', 'E']),
        key('snap-left', 'Snap Left', ['Win', 'Left']),
        key('snap-right', 'Snap Right', ['Win', 'Right']),
        key('screenshot', 'Screenshot', ['Win', 'Shift', 'S']),
        key('clipboard', 'Clipboard', ['Win', 'V']),
        key('close', 'Close Window', ['Alt', 'F4']),
        key('task-manager', 'Task Manager', ['Ctrl', 'Shift', 'Esc']),
      ],
    },
    {
      id: 'media',
      title: 'Media',
      controls: [
        key('play', 'Play / Pause', ['MediaPlayPause']),
        key('prev', 'Previous', ['MediaPrev']),
        key('next', 'Next', ['MediaNext']),
        key('vol-down', 'Volume −', ['VolumeDown']),
        key('vol-up', 'Volume +', ['VolumeUp']),
        key('mute', 'Mute', ['VolumeMute']),
      ],
    },
  ],
};

export const BLENDER_PROFILE: AppProfile = {
  id: 'blender',
  name: 'Blender',
  executables: ['blender.exe', 'blender-launcher.exe'],
  builtin: true,
  sections: [
    {
      id: 'transform',
      title: 'Transform',
      controls: [
        key('grab', 'Grab', ['G'], { hint: 'G' }),
        key('rotate', 'Rotate', ['R'], { hint: 'R' }),
        key('scale', 'Scale', ['S'], { hint: 'S' }),
        key('extrude', 'Extrude', ['E'], { hint: 'E' }),
        key('keyframe', 'Keyframe', ['I'], { hint: 'I' }),
        key('axis-x', 'X', ['X'], { hint: 'Axis / Delete' }),
        key('axis-y', 'Y', ['Y'], { hint: 'Axis' }),
        key('axis-z', 'Z', ['Z'], { hint: 'Axis / Shading pie' }),
      ],
    },
    {
      id: 'modifiers',
      title: 'Modifiers',
      controls: [
        key('ctrl', 'Ctrl', ['Ctrl'], { mode: 'latch' }),
        key('shift', 'Shift', ['Shift'], { mode: 'latch' }),
        key('alt', 'Alt', ['Alt'], { mode: 'latch' }),
        key('tab', 'Tab', ['Tab'], { hint: 'Object ⇄ Edit' }),
        key('undo', 'Undo', ['Ctrl', 'Z']),
        key('redo', 'Redo', ['Ctrl', 'Shift', 'Z']),
      ],
    },
    {
      id: 'modeling',
      title: 'Modeling',
      controls: [
        key('bevel', 'Bevel', ['Ctrl', 'B']),
        key('loop-cut', 'Loop Cut', ['Ctrl', 'R']),
        key('knife', 'Knife', ['K']),
        key('inset', 'Inset', ['I']),
        key('add', 'Add', ['Shift', 'A']),
        key('duplicate', 'Duplicate', ['Shift', 'D']),
        key('delete', 'Delete', ['X']),
        key('select-all', 'Select All', ['A']),
        key('deselect', 'Deselect', ['Alt', 'A']),
        key('vertex', 'Vertex', ['1'], { hint: 'Edit mode' }),
        key('edge', 'Edge', ['2'], { hint: 'Edit mode' }),
        key('face', 'Face', ['3'], { hint: 'Edit mode' }),
      ],
    },
    {
      id: 'modes',
      title: 'Modes',
      controls: [
        key('object-mode', 'Object Mode', ['Tab'], { hint: 'Toggles with Edit Mode', wide: true }),
        key('edit-mode', 'Edit Mode', ['Tab'], { hint: 'Toggles with Object Mode', wide: true }),
        key('sculpt-mode', 'Sculpt Mode', ['Ctrl', 'Tab'], { hint: 'Opens the mode pie', wide: true }),
      ],
    },
    {
      id: 'sculpt',
      title: 'Sculpt',
      controls: [
        key('radius', 'Radius', ['F']),
        key('strength', 'Strength', ['Shift', 'F']),
        key('invert', 'Invert', ['Ctrl'], { mode: 'hold', hint: 'Hold while stroking' }),
        key('smooth', 'Smooth', ['Shift'], { mode: 'hold', hint: 'Hold while stroking' }),
        key('mask', 'Mask', ['M']),
      ],
    },
    {
      id: 'view',
      title: 'View',
      controls: [
        key('front', 'Front', ['Num1']),
        key('right', 'Right', ['Num3']),
        key('top', 'Top', ['Num7']),
        key('camera', 'Camera', ['Num0']),
        key('ortho', 'Persp / Ortho', ['Num5']),
        key('frame', 'Frame Selected', ['NumDecimal']),
        key('frame-all', 'Frame All', ['Home']),
        key('play', 'Play', ['Space']),
      ],
    },
  ],
};

export const MAYA_PROFILE: AppProfile = {
  id: 'maya',
  name: 'Maya',
  executables: ['maya.exe', 'mayabatch.exe'],
  builtin: true,
  sections: [
    {
      id: 'tools',
      title: 'Tools',
      controls: [
        key('select', 'Select', ['Q']),
        key('move', 'Move', ['W']),
        key('rotate', 'Rotate', ['E']),
        key('scale', 'Scale', ['R']),
        key('manip', 'Manipulator', ['T']),
        key('last', 'Last Tool', ['Y']),
        key('pivot', 'Edit Pivot', ['Insert']),
        key('hotbox', 'Hotbox', ['Space'], { mode: 'hold' }),
      ],
    },
    {
      id: 'navigate',
      title: 'Navigate',
      controls: [
        key('alt', 'Alt', ['Alt'], { mode: 'latch', hint: 'Tumble / track with the Pencil' }),
        key('frame', 'Frame Selected', ['F']),
        key('frame-all', 'Frame All', ['A']),
        key('ctrl', 'Ctrl', ['Ctrl'], { mode: 'latch' }),
        key('shift', 'Shift', ['Shift'], { mode: 'latch' }),
      ],
    },
    {
      id: 'components',
      title: 'Components',
      controls: [
        key('object', 'Object', ['F8']),
        key('vertex', 'Vertex', ['F9']),
        key('edge', 'Edge', ['F10']),
        key('face', 'Face', ['F11']),
        key('uv', 'UV', ['F12']),
      ],
    },
    {
      id: 'modeling',
      title: 'Modeling',
      controls: [
        key('extrude', 'Extrude', ['Ctrl', 'E']),
        key('bevel', 'Bevel', ['Ctrl', 'B']),
        key('multicut', 'Multi-Cut', ['Ctrl', 'Shift', 'X']),
        key('duplicate', 'Duplicate', ['Ctrl', 'D']),
        key('group', 'Group', ['Ctrl', 'G']),
        key('parent', 'Parent', ['P']),
        key('unparent', 'Unparent', ['Shift', 'P']),
        key('delete', 'Delete', ['Delete']),
        key('undo', 'Undo', ['Ctrl', 'Z']),
        key('redo', 'Redo', ['Shift', 'Z']),
      ],
    },
    {
      id: 'display',
      title: 'Display',
      controls: [
        key('low', 'Rough', ['1']),
        key('medium', 'Medium', ['2']),
        key('smooth', 'Smooth', ['3']),
        key('wire', 'Wireframe', ['4']),
        key('shaded', 'Shaded', ['5']),
        key('textured', 'Textured', ['6']),
        key('lights', 'Lighting', ['7']),
      ],
    },
    {
      id: 'animation',
      title: 'Animation',
      controls: [
        key('key', 'Set Key', ['S']),
        key('key-move', 'Key Translate', ['Shift', 'W']),
        key('key-rotate', 'Key Rotate', ['Shift', 'E']),
        key('key-scale', 'Key Scale', ['Shift', 'R']),
        key('prev-frame', 'Prev Frame', ['Alt', 'Comma']),
        key('next-frame', 'Next Frame', ['Alt', 'Period']),
      ],
    },
  ],
};

export const PHOTOSHOP_PROFILE: AppProfile = {
  id: 'photoshop',
  name: 'Photoshop',
  executables: ['photoshop.exe'],
  builtin: true,
  sections: [
    {
      id: 'brush',
      title: 'Brush',
      controls: [
        stepper('size', 'Size', ['BracketLeft'], ['BracketRight'], '[ ]'),
        stepper('hardness', 'Hardness', ['Shift', 'BracketLeft'], ['Shift', 'BracketRight'], '⇧[ ⇧]'),
        key('brush', 'Brush', ['B']),
        key('eraser', 'Eraser', ['E']),
        key('eyedropper', 'Eyedropper', ['Alt'], { mode: 'hold', hint: 'Hold and tap the canvas' }),
        key('swap', 'Swap Colors', ['X']),
        key('default', 'Default Colors', ['D']),
      ],
    },
    {
      id: 'history',
      title: 'History',
      controls: [
        key('undo', 'Undo', ['Ctrl', 'Z'], { wide: true }),
        key('redo', 'Redo', ['Ctrl', 'Shift', 'Z'], { wide: true }),
      ],
    },
    {
      id: 'canvas',
      title: 'Canvas',
      controls: [
        key('zoom-in', 'Zoom In', ['Ctrl', 'Equal']),
        key('zoom-out', 'Zoom Out', ['Ctrl', 'Minus']),
        key('fit', 'Fit', ['Ctrl', '0']),
        key('actual', '100%', ['Ctrl', '1']),
        key('rotate', 'Rotate Canvas', ['R'], { mode: 'hold', hint: 'Hold and drag' }),
        key('hand', 'Pan', ['Space'], { mode: 'hold', hint: 'Hold and drag' }),
        key('fullscreen', 'Screen Mode', ['F']),
      ],
    },
    {
      id: 'tools',
      title: 'Tools',
      controls: [
        key('move', 'Move', ['V']),
        key('marquee', 'Marquee', ['M']),
        key('lasso', 'Lasso', ['L']),
        key('quick-select', 'Quick Select', ['W']),
        key('gradient', 'Gradient', ['G']),
        key('type', 'Type', ['T']),
        key('crop', 'Crop', ['C']),
        key('smudge', 'Smudge', ['R'], { hint: 'Tool group' }),
      ],
    },
    {
      id: 'layers',
      title: 'Layers',
      controls: [
        key('new-layer', 'New Layer', ['Ctrl', 'Shift', 'N']),
        key('duplicate', 'Duplicate', ['Ctrl', 'J']),
        key('merge', 'Merge Down', ['Ctrl', 'E']),
        key('transform', 'Transform', ['Ctrl', 'T']),
        key('deselect', 'Deselect', ['Ctrl', 'D']),
        key('invert', 'Invert Selection', ['Ctrl', 'Shift', 'I']),
        key('shift', 'Shift', ['Shift'], { mode: 'hold', hint: 'Straight lines' }),
      ],
    },
  ],
};

export const ZBRUSH_PROFILE: AppProfile = {
  id: 'zbrush',
  name: 'ZBrush',
  executables: ['zbrush.exe', 'zbrushcore.exe'],
  builtin: true,
  sections: [
    { id: 'brush', title: 'Brosses', controls: [key('brush', 'Brush', ['B']), key('move', 'Move', ['W']), key('scale', 'Scale', ['E']), key('rotate', 'Rotate', ['R']), key('smooth', 'Smooth', ['Shift'], { mode: 'hold' }), key('invert', 'Invert mask', ['Ctrl'], { mode: 'hold' })] },
    { id: 'view', title: 'Vue', controls: [key('frame', 'Frame', ['F']), key('perspective', 'Perspective', ['P']), key('floor', 'Floor', ['Shift', 'P']), key('solo', 'Solo', ['Tab']), key('undo', 'Undo', ['Ctrl', 'Z']), key('redo', 'Redo', ['Ctrl', 'Shift', 'Z'])] },
  ],
};

export const HOUDINI_PROFILE: AppProfile = {
  id: 'houdini',
  name: 'Houdini',
  executables: ['houdini.exe', 'houdinifx.exe', 'houdiniapprentice.exe'],
  builtin: true,
  sections: [
    { id: 'tools', title: 'Outils', controls: [key('select', 'Select', ['S']), key('translate', 'Translate', ['T']), key('rotate', 'Rotate', ['R']), key('scale', 'Scale', ['E']), key('view', 'View', ['Space'], { mode: 'hold' })] },
    { id: 'view', title: 'Vue', controls: [key('frame', 'Frame selected', ['G']), key('frame-all', 'Frame all', ['H']), key('perspective', 'Perspective', ['Num5']), key('undo', 'Undo', ['Ctrl', 'Z']), key('redo', 'Redo', ['Ctrl', 'Shift', 'Z'])] },
  ],
};

export const BUILTIN_PROFILES: AppProfile[] = [GENERIC_PROFILE, BLENDER_PROFILE, MAYA_PROFILE, ZBRUSH_PROFILE, HOUDINI_PROFILE, PHOTOSHOP_PROFILE];

/** Example of a user workspace, shipped as a template. */
export const BLENDER_WORKSPACE_TEMPLATE: AppProfile = {
  id: 'blender-workspace',
  name: 'Blender Workspace',
  executables: [],
  builtin: false,
  sections: [
    {
      id: 'custom',
      title: 'My Shortcuts',
      controls: [
        key('c-bevel', 'Bevel', ['Ctrl', 'B']),
        key('c-loop', 'Loop Cut', ['Ctrl', 'R']),
        key('c-add', 'Add', ['Shift', 'A']),
        key('c-front', 'Front', ['Num1']),
        key('c-right', 'Right', ['Num3']),
        key('c-top', 'Top', ['Num7']),
      ],
    },
  ],
};

export function profileForExecutable(exe: string, profiles: AppProfile[]): AppProfile | undefined {
  const name = exe.toLowerCase();
  return profiles.find((p) => p.executables.includes(name));
}
