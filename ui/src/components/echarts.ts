// ECharts 按需引入（T-53）：只注册用到的图表与组件，SVG 渲染。由 EChart.vue 动态加载，
// 打包时单独成块，不进首屏。
import { BarChart, LineChart } from 'echarts/charts'
import {
  GridComponent,
  LegendComponent,
  MarkLineComponent,
  TooltipComponent,
} from 'echarts/components'
import { init, use } from 'echarts/core'
import { SVGRenderer } from 'echarts/renderers'

use([BarChart, LineChart, GridComponent, LegendComponent, MarkLineComponent, TooltipComponent, SVGRenderer])

export { init }
