import { useVerge } from '@/hooks/use-verge'
import {
  getCoreCapabilities,
  normalizeCoreCapabilities,
  type ICoreCapabilities,
} from '@/services/core-capabilities'
import { useQuery } from '@/services/query-client'

/**
 * 当前内核能力:UI 灰显与接口分流的统一消费点(工单 07)。
 * 组件从这里取事实,不写 `内核 === 'verge-meow'` 之类的散落特判。
 *
 * 查询未就绪时按 mihomo 基线呈现,保证 mihomo 用户在任何加载瞬间都不出现灰显;
 * clash_core 变化即换查询键重取,内核切换后能力随之收敛。
 */
export const useCoreCapabilities = (): ICoreCapabilities => {
  const { verge } = useVerge()
  const core = verge?.clash_core

  const { data } = useQuery({
    queryKey: ['getCoreCapabilities', core],
    queryFn: getCoreCapabilities,
  })

  return normalizeCoreCapabilities(data)
}
