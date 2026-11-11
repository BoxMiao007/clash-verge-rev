import { Alert, type SxProps } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { getCoreDisplayName } from '@/services/core-capabilities'
import type { TranslationKey } from '@/types/generated/i18n-keys'

interface Props {
  /** 能力缺失时为 true 才渲染提示。 */
  active: boolean
  /** 提示文案键;文案内插 `{ core }` 为当前内核显示名。 */
  message: TranslationKey
  /** 当前内核标识(能力载荷里的 core)。 */
  core: string
  sx?: SxProps
}

/** 内核能力缺失提示:把「Alert + 内核显示名插值」的统一外观收在一处。 */
export const CapabilityNotice = ({ active, message, core, sx }: Props) => {
  const { t } = useTranslation()
  if (!active) return null
  return (
    <Alert severity="info" variant="outlined" sx={sx}>
      {t(message, { core: getCoreDisplayName(core) })}
    </Alert>
  )
}
