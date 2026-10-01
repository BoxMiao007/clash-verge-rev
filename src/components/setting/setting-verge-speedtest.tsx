import { InputAdornment, TextField } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { TooltipIcon } from '@/components/base'
import { useVerge } from '@/hooks/use-verge'
import {
  DEFAULT_SPEEDTEST_URL,
  DEFAULT_SPEEDTEST_WINDOW_SECS,
  MIN_SPEEDTEST_DURATION_SECS,
  clampSpeedtestDurationSecs,
  resolveSpeedtestDurationSecs,
} from '@/utils/speed'

import { GuardState } from './mods/guard-state'
import { SettingItem, SettingList } from './mods/setting-comp'

interface Props {
  onError?: (err: Error) => void
}

const SettingVergeSpeedtest = ({ onError }: Props) => {
  const { t } = useTranslation()

  const { verge, patchVerge, mutateVerge } = useVerge()
  const { default_speedtest_url, default_speedtest_duration } = verge ?? {}

  // URL 为自由文本:输入过程只改草稿,失焦才落盘(空串回落内置默认值)。
  const [urlDraft, setUrlDraft] = useState<string | null>(null)

  const onChangeData = (patch: any) => {
    mutateVerge({ ...verge, ...patch }, false)
  }

  const onCommitUrl = useLockFn(async () => {
    if (urlDraft === null) return
    const url = urlDraft.trim()
    setUrlDraft(null)
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

  return (
    <SettingList title={t('settings.components.verge.speedtest.title')}>
      <SettingItem
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
      </SettingItem>

      <SettingItem
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
    </SettingList>
  )
}

export default SettingVergeSpeedtest
