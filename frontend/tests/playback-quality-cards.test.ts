import assert from 'node:assert/strict';
import { test } from 'node:test';
import { qualityMenuPlan, qualityOrder } from '../src/services/playbackQuality.ts';
import type {
  QualityChoiceView,
  QualityDetailsView,
  QualityTrialView,
} from '../src/services/player.ts';

function row(
  quality: QualityChoiceView['quality'],
  availability: QualityChoiceView['availability'] = 'available',
  trialPhase: QualityChoiceView['trialPhase'] = null,
): QualityChoiceView {
  return {
    quality,
    availability,
    trialPhase,
    access: 'unknown',
    sizeBytes: null,
    remainingMs: null,
  };
}
function trial(
  right: QualityTrialView['right'],
  phase: QualityTrialView['phase'],
): QualityTrialView {
  return {
    right,
    phase,
    operation: 'fixture',
    revision: 'fixture',
    acknowledged: false,
    canUse: false,
    readFailed: false,
    remainingMs: null,
  };
}
function details(
  choices: QualityChoiceView[],
  vivid?: boolean,
  trials: QualityTrialView[] = [],
): QualityDetailsView {
  return {
    epoch: '1',
    playId: 'p',
    songId: '1',
    revision: 'r',
    choices,
    trials,
    trialsDamaged: false,
    status: { privilege: 'ready', sizes: 'ready', subscriber: 'ready', useRights: {} },
    refreshAfterMs: null,
    ...(vivid === undefined ? {} : { presentation: { kind: 'cards', vivid } }),
  };
}
test('the new card row begins with actual Sky and Master offers, above optionally allowed Vivid', () => {
  const choices = qualityOrder.map((quality) => row(quality));
  assert.deepEqual(qualityMenuPlan(details(choices, true)), {
    main: ['sky', 'jymaster', 'vivid', 'jyeffect', 'hires', 'lossless', 'exhigh', 'standard'],
    compatibility: [],
    pair: ['sky', 'jymaster'],
  });
  assert.deepEqual(qualityMenuPlan(details(choices, false)).main, [
    'sky',
    'jymaster',
    'jyeffect',
    'hires',
    'lossless',
    'exhigh',
    'standard',
  ]);
  assert.deepEqual(qualityMenuPlan(details(choices)), {
    main: qualityOrder.slice(0, 8),
    compatibility: qualityOrder.slice(8),
    pair: undefined,
  });
});
test('a single verified card stays vertical; unknown offers do not fabricate a pair', () => {
  for (const availability of ['unavailable', 'unknown'] as const) {
    assert.deepEqual(
      qualityMenuPlan(details([row('sky', availability), row('jymaster'), row('standard')], false)),
      {
        main: ['jymaster', 'standard'],
        compatibility: [],
        pair: undefined,
      },
    );
  }
});
test('a changed capability or cohort cannot hide current audio or active and unresolved trials', () => {
  const plan = qualityMenuPlan(
    details(
      [
        row('dolby', 'unavailable'),
        row('sky', 'unknown', 'active'),
        row('vivid', 'unavailable'),
        row('jymaster', 'unavailable'),
        row('jyeffect', 'unavailable'),
        row('standard'),
      ],
      false,
      [trial('vivid', 'uncertain'), trial('jymaster', 'active'), trial('jyeffect', 'expired')],
    ),
    'dolby',
  );
  assert.deepEqual(plan, {
    main: ['sky', 'jymaster', 'vivid', 'standard'],
    compatibility: ['dolby'],
    pair: undefined,
  });
  assert.deepEqual(qualityMenuPlan(details([row('standard')], false), 'vivid').main, [
    'vivid',
    'standard',
  ]);
});
