/**
 * Creative app profiles. A profile is pure data: labelled key combinations grouped in sections.
 * The Mac agent reports the bundle identifier of the frontmost application
 * (e.g. `org.blenderfoundation.blender`); the iPad maps it to a profile.
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
  /**
   * Lower-case macOS bundle identifiers (or prefixes, for versioned apps such as
   * `com.autodesk.maya.2025`) that activate this profile automatically.
   */
  bundleIds: string[];
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
  id: 'macos',
  name: 'macOS',
  bundleIds: [],
  builtin: true,
  sections: [
    {
      id: 'edit',
      title: 'Edit',
      controls: [
        key('undo', 'Undo', ['Cmd', 'Z']),
        key('redo', 'Redo', ['Cmd', 'Shift', 'Z']),
        key('cut', 'Cut', ['Cmd', 'X']),
        key('copy', 'Copy', ['Cmd', 'C']),
        key('paste', 'Paste', ['Cmd', 'V']),
        key('select-all', 'Select All', ['Cmd', 'A']),
        key('save', 'Save', ['Cmd', 'S']),
        key('find', 'Find', ['Cmd', 'F']),
      ],
    },
    {
      id: 'system',
      title: 'macOS',
      controls: [
        key('switch', 'Switch App', ['Cmd', 'Tab']),
        key('mission-control', 'Mission Control', ['Control', 'Up']),
        key('app-windows', 'App Windows', ['Control', 'Down']),
        key('desktop', 'Show Desktop', ['F11']),
        key('space-left', 'Previous Space', ['Control', 'Left']),
        key('space-right', 'Next Space', ['Control', 'Right']),
        key('spotlight', 'Spotlight', ['Cmd', 'Space']),
        key('screenshot', 'Screenshot', ['Cmd', 'Shift', '5']),
        key('close', 'Close Window', ['Cmd', 'W']),
        key('minimize', 'Minimize', ['Cmd', 'M']),
        key('hide', 'Hide App', ['Cmd', 'H']),
        key('quit', 'Quit App', ['Cmd', 'Q']),
        key('force-quit', 'Force Quit', ['Cmd', 'Option', 'Esc']),
        key('lock', 'Lock Screen', ['Control', 'Cmd', 'Q']),
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
  bundleIds: ['org.blenderfoundation.blender'],
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
        key('cmd', '⌘ Cmd', ['Cmd'], { mode: 'latch' }),
        key('ctrl', '⌃ Control', ['Control'], { mode: 'latch' }),
        key('shift', '⇧ Shift', ['Shift'], { mode: 'latch' }),
        key('alt', '⌥ Option', ['Option'], { mode: 'latch' }),
        key('tab', 'Tab', ['Tab'], { hint: 'Object ⇄ Edit' }),
        key('undo', 'Undo', ['Cmd', 'Z']),
        key('redo', 'Redo', ['Cmd', 'Shift', 'Z']),
      ],
    },
    {
      id: 'modeling',
      title: 'Modeling',
      controls: [
        key('bevel', 'Bevel', ['Control', 'B']),
        key('loop-cut', 'Loop Cut', ['Control', 'R']),
        key('knife', 'Knife', ['K']),
        key('inset', 'Inset', ['I']),
        key('add', 'Add', ['Shift', 'A']),
        key('duplicate', 'Duplicate', ['Shift', 'D']),
        key('delete', 'Delete', ['X']),
        key('select-all', 'Select All', ['A']),
        key('deselect', 'Deselect', ['Option', 'A']),
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
        key('sculpt-mode', 'Sculpt Mode', ['Control', 'Tab'], { hint: 'Opens the mode pie', wide: true }),
      ],
    },
    {
      id: 'sculpt',
      title: 'Sculpt',
      controls: [
        key('radius', 'Radius', ['F']),
        key('strength', 'Strength', ['Shift', 'F']),
        key('invert', 'Invert', ['Control'], { mode: 'hold', hint: 'Hold while stroking' }),
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
  bundleIds: ['com.autodesk.maya'],
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
        key('alt', '⌥ Option', ['Option'], { mode: 'latch', hint: 'Tumble / track with the Pencil' }),
        key('frame', 'Frame Selected', ['F']),
        key('frame-all', 'Frame All', ['A']),
        key('cmd', '⌘ Cmd', ['Cmd'], { mode: 'latch' }),
        key('shift', '⇧ Shift', ['Shift'], { mode: 'latch' }),
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
        key('extrude', 'Extrude', ['Cmd', 'E']),
        key('bevel', 'Bevel', ['Cmd', 'B']),
        key('multicut', 'Multi-Cut', ['Cmd', 'Shift', 'X']),
        key('duplicate', 'Duplicate', ['Cmd', 'D']),
        key('group', 'Group', ['Cmd', 'G']),
        key('parent', 'Parent', ['P']),
        key('unparent', 'Unparent', ['Shift', 'P']),
        key('delete', 'Delete', ['Delete']),
        key('undo', 'Undo', ['Cmd', 'Z']),
        key('redo', 'Redo', ['Cmd', 'Shift', 'Z']),
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
        key('prev-frame', 'Prev Frame', ['Option', 'Comma']),
        key('next-frame', 'Next Frame', ['Option', 'Period']),
      ],
    },
  ],
};

export const PHOTOSHOP_PROFILE: AppProfile = {
  id: 'photoshop',
  name: 'Photoshop',
  bundleIds: ['com.adobe.photoshop'],
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
        key('eyedropper', 'Eyedropper', ['Option'], { mode: 'hold', hint: 'Hold and tap the canvas' }),
        key('swap', 'Swap Colors', ['X']),
        key('default', 'Default Colors', ['D']),
      ],
    },
    {
      id: 'history',
      title: 'History',
      controls: [
        key('undo', 'Undo', ['Cmd', 'Z'], { wide: true }),
        key('redo', 'Redo', ['Cmd', 'Shift', 'Z'], { wide: true }),
      ],
    },
    {
      id: 'canvas',
      title: 'Canvas',
      controls: [
        key('zoom-in', 'Zoom In', ['Cmd', 'Equal']),
        key('zoom-out', 'Zoom Out', ['Cmd', 'Minus']),
        key('fit', 'Fit', ['Cmd', '0']),
        key('actual', '100%', ['Cmd', '1']),
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
        key('new-layer', 'New Layer', ['Cmd', 'Shift', 'N']),
        key('duplicate', 'Duplicate', ['Cmd', 'J']),
        key('merge', 'Merge Down', ['Cmd', 'E']),
        key('transform', 'Transform', ['Cmd', 'T']),
        key('deselect', 'Deselect', ['Cmd', 'D']),
        key('invert', 'Invert Selection', ['Cmd', 'Shift', 'I']),
        key('shift', 'Shift', ['Shift'], { mode: 'hold', hint: 'Straight lines' }),
      ],
    },
  ],
};

export const ZBRUSH_PROFILE: AppProfile = {
  id: 'zbrush',
  name: 'ZBrush',
  bundleIds: ['com.pixologic.zbrush', 'com.maxon.zbrush'],
  builtin: true,
  sections: [
    { id: 'brush', title: 'Brosses', controls: [key('brush', 'Brush', ['B']), key('move', 'Move', ['W']), key('scale', 'Scale', ['E']), key('rotate', 'Rotate', ['R']), key('smooth', 'Smooth', ['Shift'], { mode: 'hold' }), key('invert', 'Invert mask', ['Cmd'], { mode: 'hold' })] },
    { id: 'view', title: 'Vue', controls: [key('frame', 'Frame', ['F']), key('perspective', 'Perspective', ['P']), key('floor', 'Floor', ['Shift', 'P']), key('solo', 'Solo', ['Tab']), key('undo', 'Undo', ['Cmd', 'Z']), key('redo', 'Redo', ['Cmd', 'Shift', 'Z'])] },
  ],
};

export const HOUDINI_PROFILE: AppProfile = {
  id: 'houdini',
  name: 'Houdini',
  bundleIds: ['com.sidefx.houdini'],
  builtin: true,
  sections: [
    { id: 'tools', title: 'Outils', controls: [key('select', 'Select', ['S']), key('translate', 'Translate', ['T']), key('rotate', 'Rotate', ['R']), key('scale', 'Scale', ['E']), key('view', 'View', ['Space'], { mode: 'hold' })] },
    { id: 'view', title: 'Vue', controls: [key('frame', 'Frame selected', ['G']), key('frame-all', 'Frame all', ['H']), key('perspective', 'Perspective', ['Num5']), key('undo', 'Undo', ['Cmd', 'Z']), key('redo', 'Redo', ['Cmd', 'Shift', 'Z'])] },
  ],
};

export const BUILTIN_PROFILES: AppProfile[] = [GENERIC_PROFILE, BLENDER_PROFILE, MAYA_PROFILE, ZBRUSH_PROFILE, HOUDINI_PROFILE, PHOTOSHOP_PROFILE];

/** Example of a user workspace, shipped as a template. */
export const BLENDER_WORKSPACE_TEMPLATE: AppProfile = {
  id: 'blender-workspace',
  name: 'Blender Workspace',
  bundleIds: [],
  builtin: false,
  sections: [
    {
      id: 'custom',
      title: 'My Shortcuts',
      controls: [
        key('c-bevel', 'Bevel', ['Control', 'B']),
        key('c-loop', 'Loop Cut', ['Control', 'R']),
        key('c-add', 'Add', ['Shift', 'A']),
        key('c-front', 'Front', ['Num1']),
        key('c-right', 'Right', ['Num3']),
        key('c-top', 'Top', ['Num7']),
      ],
    },
  ],
};

/** Profile for the frontmost Mac application, matched by bundle identifier or prefix. */
export function profileForApp(bundleId: string, profiles: AppProfile[]): AppProfile | undefined {
  const id = bundleId.toLowerCase();
  return profiles.find((p) => (p.bundleIds ?? []).some((prefix) => id === prefix || id.startsWith(prefix)));
}
