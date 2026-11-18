import {
  Box,
  InputAdornment,
  MenuItem,
  TextField,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { TooltipIcon } from '@/components/base'
import { useVerge } from '@/hooks/use-verge'
import {
  DEFAULT_SPEEDTEST_CONCURRENCY,
  DEFAULT_SPEEDTEST_URL,
  DEFAULT_SPEEDTEST_WINDOW_SECS,
  MIN_SPEEDTEST_DURATION_SECS,
  SPEEDTEST_URL_PRESETS,
  clampSpeedtestConcurrency,
  clampSpeedtestDurationSecs,
  resolveSpeedtestConcurrency,
  resolveSpeedtestDurationSecs,
  resolveSpeedtestMaxMb,
} from '@/utils/speed'

import { GuardState } from './mods/guard-state'
import { SettingItem, SettingList } from './mods/setting-comp'

interface Props {
  onError?: (err: Error) => void
}

const SettingVergeSpeedtest = ({ onError }: Props) => {
  const { t } = useTranslation()

  const { verge, patchVerge, mutateVerge } = useVerge()
  const {
    default_speedtest_url,
    default_speedtest_duration,
    default_speedtest_max_mb,
    default_speedtest_concurrency,
  } = verge ?? {}

  // URL 为自由文本:输入过程只改草稿,失焦才落盘(空串回落内置默认值)。
  const [urlDraft, setUrlDraft] = useState<string | null>(null)

  const onChangeData = (patch: any) => {
    mutateVerge({ ...verge, ...patch })
  }

  const commitUrl = useLockFn(async (raw: string) => {
    setUrlDraft(null)
    const url = raw.trim()
    // 无变化不落盘,避免每次失焦都重写配置。
    if (url === (default_speedtest_url ?? '').trim()) return
    onChangeData({ default_speedtest_url: url })
    try {
      await patchVerge({ default_speedtest_url: url })
    } catch (err) {
      // 落盘失败回滚本地缓存到配置值。
      onChangeData({ default_speedtest_url: default_speedtest_url ?? '' })
      onError?.(err as Error)
    }
  })

  const onCommitUrl = () => {
    if (urlDraft === null) return
    void commitUrl(urlDraft)
  }

  const onSelectPreset = useLockFn(async (id: string) => {
    if (id === 'custom') return
    const preset = SPEEDTEST_URL_PRESETS.find((item) => item.id === id)
    if (preset) await commitUrl(preset.url)
  })

  const currentUrl = default_speedtest_url ?? ''
  const currentPresetId =
    SPEEDTEST_URL_PRESETS.find((preset) => preset.url === currentUrl.trim())
      ?.id ?? 'custom'

  return (
    <SettingList title={t('settings.components.verge.speedtest.title')}>
      <SettingItem
        vertical
        data-setting-target="settings.components.verge.speedtest.fields.speedtestUrl"
        label={t('settings.components.verge.speedtest.fields.speedtestUrl')}
        extra={
          <TooltipIcon
            title={t(
              'settings.components.verge.speedtest.tooltips.speedtestUrl',
            )}
            sx={{ opacity: '0.7' }}
          />
        }
      >
        <Box sx={{ display: 'flex', gap: 1, alignItems: 'center' }}>
          <TextField
            select
            size="small"
            sx={{ width: 150 }}
            value={currentPresetId}
            onChange={(e) => void onSelectPreset(e.target.value)}
          >
            {SPEEDTEST_URL_PRESETS.map((preset) => (
              <MenuItem key={preset.id} value={preset.id}>
                {t(preset.labelKey)}
              </MenuItem>
            ))}
            <MenuItem value="custom">
              {t(
                'settings.components.verge.speedtest.urlPresets.custom',
              )}
            </MenuItem>
          </TextField>
          <TextField
            autoComplete="new-password"
            size="small"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            sx={{ width: 250 }}
            value={urlDraft ?? default_speedtest_url ?? ''}
            placeholder={DEFAULT_SPEEDTEST_URL}
            onChange={(e) => setUrlDraft(e.target.value)}
            onBlur={onCommitUrl}
          />
        </Box>
      </SettingItem>

      <SettingItem
        data-setting-target="settings.components.verge.speedtest.fields.speedtestDuration"
        label={t(
          'settings.components.verge.speedtest.fields.speedtestDuration',
        )}
        extra={
          <TooltipIcon
            title={t(
              'settings.components.verge.speedtest.tooltips.speedtestDuration',
            )}
            sx={{ opacity: '0.7' }}
          />
        }
      >
        <GuardState
          value={resolveSpeedtestDurationSecs(default_speedtest_duration)}
          onCatch={onError}
          onFormat={(e: any) => {
            const parsed = parseInt(e.target.value, 10)
            return Number.isFinite(parsed)
              ? clampSpeedtestDurationSecs(parsed)
              : MIN_SPEEDTEST_DURATION_SECS
          }}
          onChange={(e) => onChangeData({ default_speedtest_duration: e })}
          onGuard={(e) => patchVerge({ default_speedtest_duration: e })}
        >
          <TextField
            autoComplete="new-password"
            size="small"
            type="number"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            sx={{ width: 250 }}
            placeholder={String(DEFAULT_SPEEDTEST_WINDOW_SECS)}
            slotProps={{
              input: {
                endAdornment: (
                  <InputAdornment position="end">
                    {t('shared.units.seconds')}
                  </InputAdornment>
                ),
              },
            }}
          />
        </GuardState>
      </SettingItem>

      <SettingItem
        data-setting-target="settings.components.verge.speedtest.fields.speedtestMaxMb"
        label={t('settings.components.verge.speedtest.fields.speedtestMaxMb')}
        extra={
          <TooltipIcon
            title={t(
              'settings.components.verge.speedtest.tooltips.speedtestMaxMb',
            )}
            sx={{ opacity: '0.7' }}
          />
        }
      >
        {/* 流量上限与时长同用即时夹取:0 是合法输入(不限),非法输入归 0。 */}
        <GuardState
          value={resolveSpeedtestMaxMb(default_speedtest_max_mb)}
          onCatch={onError}
          onFormat={(e: any) => {
            const parsed = parseInt(e.target.value, 10)
            return Number.isFinite(parsed) ? resolveSpeedtestMaxMb(parsed) : 0
          }}
          onChange={(e) => onChangeData({ default_speedtest_max_mb: e })}
          onGuard={(e) => patchVerge({ default_speedtest_max_mb: e })}
        >
          <TextField
            autoComplete="new-password"
            size="small"
            type="number"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            sx={{ width: 250 }}
            placeholder="0"
            slotProps={{
              input: {
                endAdornment: (
                  <InputAdornment position="end">
                    {t('shared.units.megabytes')}
                  </InputAdornment>
                ),
              },
            }}
          />
        </GuardState>
      </SettingItem>

      <SettingItem
        data-setting-target="settings.components.verge.speedtest.fields.speedtestConcurrency"
        label={t(
          'settings.components.verge.speedtest.fields.speedtestConcurrency',
        )}
        extra={
          <TooltipIcon
            title={t(
              'settings.components.verge.speedtest.tooltips.speedtestConcurrency',
            )}
            sx={{ opacity: '0.7' }}
          />
        }
      >
        <GuardState
          value={resolveSpeedtestConcurrency(default_speedtest_concurrency)}
          onCatch={onError}
          onFormat={(e: any) => {
            const parsed = parseInt(e.target.value, 10)
            return Number.isFinite(parsed)
              ? clampSpeedtestConcurrency(parsed)
              : DEFAULT_SPEEDTEST_CONCURRENCY
          }}
          onChange={(e) => onChangeData({ default_speedtest_concurrency: e })}
          onGuard={(e) => patchVerge({ default_speedtest_concurrency: e })}
        >
          <TextField
            autoComplete="new-password"
            size="small"
            type="number"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            sx={{ width: 250 }}
            placeholder={String(DEFAULT_SPEEDTEST_CONCURRENCY)}
          />
        </GuardState>
      </SettingItem>
    </SettingList>
  )
}

export default SettingVergeSpeedtest
