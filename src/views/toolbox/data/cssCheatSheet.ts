export interface CssProperty {
  property: string
  description: string
  values: string[]
  example: string
  category: string
}

export const cssProperties: CssProperty[] = [
  // 布局
  {
    property: 'display',
    description: '定义元素的显示类型',
    values: ['block', 'inline', 'inline-block', 'flex', 'grid', 'none', 'table', 'inline-flex'],
    example: 'display: flex;',
    category: '布局',
  },
  {
    property: 'position',
    description: '定义元素的定位方式',
    values: ['static', 'relative', 'absolute', 'fixed', 'sticky'],
    example: 'position: relative;',
    category: '布局',
  },
  {
    property: 'float',
    description: '定义元素的浮动',
    values: ['left', 'right', 'none'],
    example: 'float: left;',
    category: '布局',
  },
  {
    property: 'clear',
    description: '清除浮动',
    values: ['left', 'right', 'both', 'none'],
    example: 'clear: both;',
    category: '布局',
  },
  {
    property: 'overflow',
    description: '定义内容溢出时的行为',
    values: ['visible', 'hidden', 'scroll', 'auto'],
    example: 'overflow: hidden;',
    category: '布局',
  },
  {
    property: 'z-index',
    description: '定义元素的堆叠顺序',
    values: ['auto', '<integer>'],
    example: 'z-index: 10;',
    category: '布局',
  },

  // Flexbox
  {
    property: 'flex-direction',
    description: '定义主轴方向',
    values: ['row', 'row-reverse', 'column', 'column-reverse'],
    example: 'flex-direction: column;',
    category: 'Flexbox',
  },
  {
    property: 'flex-wrap',
    description: '定义是否换行',
    values: ['nowrap', 'wrap', 'wrap-reverse'],
    example: 'flex-wrap: wrap;',
    category: 'Flexbox',
  },
  {
    property: 'justify-content',
    description: '定义主轴对齐方式',
    values: ['flex-start', 'flex-end', 'center', 'space-between', 'space-around', 'space-evenly'],
    example: 'justify-content: center;',
    category: 'Flexbox',
  },
  {
    property: 'align-items',
    description: '定义交叉轴对齐方式',
    values: ['flex-start', 'flex-end', 'center', 'stretch', 'baseline'],
    example: 'align-items: center;',
    category: 'Flexbox',
  },
  {
    property: 'align-self',
    description: '定义单个元素的交叉轴对齐方式',
    values: ['auto', 'flex-start', 'flex-end', 'center', 'stretch', 'baseline'],
    example: 'align-self: flex-end;',
    category: 'Flexbox',
  },
  {
    property: 'flex',
    description: '定义弹性项目的伸缩性',
    values: ['<flex-grow> <flex-shrink> <flex-basis>'],
    example: 'flex: 1;',
    category: 'Flexbox',
  },
  {
    property: 'gap',
    description: '定义网格或弹性布局的间距',
    values: ['<length>', '<percentage>'],
    example: 'gap: 16px;',
    category: 'Flexbox',
  },

  // Grid
  {
    property: 'grid-template-columns',
    description: '定义网格列',
    values: ['<track-size>', 'repeat()', 'auto', 'minmax()'],
    example: 'grid-template-columns: 1fr 2fr;',
    category: 'Grid',
  },
  {
    property: 'grid-template-rows',
    description: '定义网格行',
    values: ['<track-size>', 'repeat()', 'auto', 'minmax()'],
    example: 'grid-template-rows: auto 1fr;',
    category: 'Grid',
  },
  {
    property: 'grid-column',
    description: '定义元素跨越的列',
    values: ['<start> / <end>', 'span <n>'],
    example: 'grid-column: 1 / 3;',
    category: 'Grid',
  },
  {
    property: 'grid-row',
    description: '定义元素跨越的行',
    values: ['<start> / <end>', 'span <n>'],
    example: 'grid-row: 1 / 2;',
    category: 'Grid',
  },

  // 盒模型
  {
    property: 'width',
    description: '定义元素的宽度',
    values: ['auto', '<length>', '<percentage>', 'fit-content', 'min-content', 'max-content'],
    example: 'width: 100%;',
    category: '盒模型',
  },
  {
    property: 'height',
    description: '定义元素的高度',
    values: ['auto', '<length>', '<percentage>'],
    example: 'height: 200px;',
    category: '盒模型',
  },
  {
    property: 'margin',
    description: '定义外边距',
    values: ['<length>', '<percentage>', 'auto'],
    example: 'margin: 10px 20px;',
    category: '盒模型',
  },
  {
    property: 'padding',
    description: '定义内边距',
    values: ['<length>', '<percentage>'],
    example: 'padding: 10px;',
    category: '盒模型',
  },
  {
    property: 'border',
    description: '定义边框',
    values: ['<width> <style> <color>'],
    example: 'border: 1px solid #ccc;',
    category: '盒模型',
  },
  {
    property: 'border-radius',
    description: '定义圆角',
    values: ['<length>', '<percentage>'],
    example: 'border-radius: 8px;',
    category: '盒模型',
  },
  {
    property: 'box-shadow',
    description: '定义阴影',
    values: ['<offset-x> <offset-y> <blur> <spread> <color>'],
    example: 'box-shadow: 0 2px 4px rgba(0,0,0,0.1);',
    category: '盒模型',
  },
  {
    property: 'box-sizing',
    description: '定义盒模型计算方式',
    values: ['content-box', 'border-box'],
    example: 'box-sizing: border-box;',
    category: '盒模型',
  },

  // 文字
  {
    property: 'font-family',
    description: '定义字体',
    values: ['<family-name>', 'serif', 'sans-serif', 'monospace'],
    example: 'font-family: "Arial", sans-serif;',
    category: '文字',
  },
  {
    property: 'font-size',
    description: '定义字号',
    values: ['<length>', '<percentage>', 'small', 'medium', 'large'],
    example: 'font-size: 16px;',
    category: '文字',
  },
  {
    property: 'font-weight',
    description: '定义字体粗细',
    values: ['normal', 'bold', '100-900'],
    example: 'font-weight: bold;',
    category: '文字',
  },
  {
    property: 'font-style',
    description: '定义字体样式',
    values: ['normal', 'italic', 'oblique'],
    example: 'font-style: italic;',
    category: '文字',
  },
  {
    property: 'text-align',
    description: '定义文本对齐方式',
    values: ['left', 'right', 'center', 'justify'],
    example: 'text-align: center;',
    category: '文字',
  },
  {
    property: 'text-decoration',
    description: '定义文本装饰',
    values: ['none', 'underline', 'overline', 'line-through'],
    example: 'text-decoration: underline;',
    category: '文字',
  },
  {
    property: 'text-transform',
    description: '定义文本转换',
    values: ['none', 'uppercase', 'lowercase', 'capitalize'],
    example: 'text-transform: uppercase;',
    category: '文字',
  },
  {
    property: 'line-height',
    description: '定义行高',
    values: ['normal', '<number>', '<length>', '<percentage>'],
    example: 'line-height: 1.5;',
    category: '文字',
  },
  {
    property: 'letter-spacing',
    description: '定义字间距',
    values: ['normal', '<length>'],
    example: 'letter-spacing: 2px;',
    category: '文字',
  },
  {
    property: 'word-spacing',
    description: '定义词间距',
    values: ['normal', '<length>'],
    example: 'word-spacing: 4px;',
    category: '文字',
  },
  {
    property: 'white-space',
    description: '定义空白处理方式',
    values: ['normal', 'nowrap', 'pre', 'pre-wrap', 'pre-line'],
    example: 'white-space: nowrap;',
    category: '文字',
  },
  {
    property: 'text-overflow',
    description: '定义文本溢出处理',
    values: ['clip', 'ellipsis'],
    example: 'text-overflow: ellipsis;',
    category: '文字',
  },

  // 颜色与背景
  {
    property: 'color',
    description: '定义文本颜色',
    values: ['<color>', 'inherit'],
    example: 'color: #333;',
    category: '颜色与背景',
  },
  {
    property: 'background',
    description: '定义背景',
    values: ['<color>', '<image>', '<position>', '<size>'],
    example: 'background: #fff url("bg.jpg") no-repeat center;',
    category: '颜色与背景',
  },
  {
    property: 'background-color',
    description: '定义背景颜色',
    values: ['<color>', 'transparent'],
    example: 'background-color: #f0f0f0;',
    category: '颜色与背景',
  },
  {
    property: 'background-image',
    description: '定义背景图片',
    values: ['url()', 'linear-gradient()', 'radial-gradient()', 'none'],
    example: 'background-image: url("bg.jpg");',
    category: '颜色与背景',
  },
  {
    property: 'background-size',
    description: '定义背景图片大小',
    values: ['auto', 'cover', 'contain', '<length>', '<percentage>'],
    example: 'background-size: cover;',
    category: '颜色与背景',
  },
  {
    property: 'background-position',
    description: '定义背景图片位置',
    values: ['<position>', 'center', 'top', 'bottom', 'left', 'right'],
    example: 'background-position: center;',
    category: '颜色与背景',
  },
  {
    property: 'opacity',
    description: '定义透明度',
    values: ['<alpha-value>'],
    example: 'opacity: 0.8;',
    category: '颜色与背景',
  },

  // 变换与动画
  {
    property: 'transform',
    description: '定义2D/3D变换',
    values: ['translate()', 'rotate()', 'scale()', 'skew()', 'matrix()'],
    example: 'transform: rotate(45deg);',
    category: '变换与动画',
  },
  {
    property: 'transition',
    description: '定义过渡效果',
    values: ['<property> <duration> <timing-function> <delay>'],
    example: 'transition: all 0.3s ease;',
    category: '变换与动画',
  },
  {
    property: 'animation',
    description: '定义动画',
    values: ['<name> <duration> <timing-function> <delay> <iteration-count> <direction>'],
    example: 'animation: fadeIn 1s ease-in-out;',
    category: '变换与动画',
  },

  // 其他
  {
    property: 'cursor',
    description: '定义鼠标指针样式',
    values: ['default', 'pointer', 'text', 'move', 'not-allowed', 'grab'],
    example: 'cursor: pointer;',
    category: '其他',
  },
  {
    property: 'visibility',
    description: '定义元素可见性',
    values: ['visible', 'hidden', 'collapse'],
    example: 'visibility: hidden;',
    category: '其他',
  },
  {
    property: 'opacity',
    description: '定义透明度',
    values: ['0-1'],
    example: 'opacity: 0.5;',
    category: '其他',
  },
  {
    property: 'pointer-events',
    description: '定义元素是否响应鼠标事件',
    values: ['auto', 'none'],
    example: 'pointer-events: none;',
    category: '其他',
  },
  {
    property: 'user-select',
    description: '定义用户是否能选择文本',
    values: ['auto', 'none', 'text', 'all'],
    example: 'user-select: none;',
    category: '其他',
  },
]

export const cssCategories = [...new Set(cssProperties.map(p => p.category))]
