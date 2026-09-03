export interface RegexPattern {
  pattern: string
  description: string
  example: string
  matches: string[]
  category: string
}

export const regexPatterns: RegexPattern[] = [
  // 基础元字符
  {
    pattern: '.',
    description: '匹配任意单个字符（除换行符）',
    example: 'a.c',
    matches: ['abc', 'a1c', 'a-c'],
    category: '基础元字符',
  },
  {
    pattern: '\\d',
    description: '匹配数字 [0-9]',
    example: '\\d{3}',
    matches: ['123', '456', '789'],
    category: '基础元字符',
  },
  {
    pattern: '\\D',
    description: '匹配非数字 [^0-9]',
    example: '\\D+',
    matches: ['abc', 'Hello', '!@#'],
    category: '基础元字符',
  },
  {
    pattern: '\\w',
    description: '匹配单词字符 [a-zA-Z0-9_]',
    example: '\\w+',
    matches: ['hello', 'world_123', 'test'],
    category: '基础元字符',
  },
  {
    pattern: '\\W',
    description: '匹配非单词字符',
    example: '\\W',
    matches: ['!', '@', '#', ' '],
    category: '基础元字符',
  },
  {
    pattern: '\\s',
    description: '匹配空白字符',
    example: '\\s+',
    matches: [' ', '\t', '\n'],
    category: '基础元字符',
  },
  {
    pattern: '\\S',
    description: '匹配非空白字符',
    example: '\\S+',
    matches: ['hello', '123', '!@#'],
    category: '基础元字符',
  },
  {
    pattern: '\\b',
    description: '匹配单词边界',
    example: '\\bword\\b',
    matches: ['word', 'a word here'],
    category: '基础元字符',
  },

  // 量词
  {
    pattern: '*',
    description: '匹配前面的表达式0次或多次',
    example: 'ab*c',
    matches: ['ac', 'abc', 'abbc'],
    category: '量词',
  },
  {
    pattern: '+',
    description: '匹配前面的表达式1次或多次',
    example: 'ab+c',
    matches: ['abc', 'abbc', 'abbbc'],
    category: '量词',
  },
  {
    pattern: '?',
    description: '匹配前面的表达式0次或1次',
    example: 'colou?r',
    matches: ['color', 'colour'],
    category: '量词',
  },
  {
    pattern: '{n}',
    description: '匹配前面的表达式恰好n次',
    example: 'a{3}',
    matches: ['aaa'],
    category: '量词',
  },
  {
    pattern: '{n,}',
    description: '匹配前面的表达式至少n次',
    example: 'a{2,}',
    matches: ['aa', 'aaa', 'aaaa'],
    category: '量词',
  },
  {
    pattern: '{n,m}',
    description: '匹配前面的表达式n到m次',
    example: 'a{2,4}',
    matches: ['aa', 'aaa', 'aaaa'],
    category: '量词',
  },

  // 字符类
  {
    pattern: '[abc]',
    description: '匹配方括号中的任意字符',
    example: '[aeiou]',
    matches: ['a', 'e', 'i', 'o', 'u'],
    category: '字符类',
  },
  {
    pattern: '[^abc]',
    description: '匹配不在方括号中的任意字符',
    example: '[^0-9]',
    matches: ['a', 'b', 'c', '!'],
    category: '字符类',
  },
  {
    pattern: '[a-z]',
    description: '匹配指定范围内的字符',
    example: '[a-zA-Z]',
    matches: ['a', 'Z', 'm'],
    category: '字符类',
  },

  // 分组和引用
  {
    pattern: '(abc)',
    description: '捕获组，匹配并捕获',
    example: '(\\d{4})-(\\d{2})-(\\d{2})',
    matches: ['2024-01-15'],
    category: '分组和引用',
  },
  {
    pattern: '(?:abc)',
    description: '非捕获组，匹配但不捕获',
    example: '(?:https?|ftp)://',
    matches: ['http://', 'https://', 'ftp://'],
    category: '分组和引用',
  },
  {
    pattern: '(?=abc)',
    description: '正向前瞻，匹配后面是abc的位置',
    example: '\\d+(?=px)',
    matches: ['10px', '20px'],
    category: '分组和引用',
  },
  {
    pattern: '(?!abc)',
    description: '负向前瞻，匹配后面不是abc的位置',
    example: '\\d+(?!px)',
    matches: ['10em', '20%'],
    category: '分组和引用',
  },
  {
    pattern: '(?<=abc)',
    description: '正向后顾，匹配前面是abc的位置',
    example: '(?<=\\$)\\d+',
    matches: ['$100', '$50'],
    category: '分组和引用',
  },
  {
    pattern: '(?<!abc)',
    description: '负向后顾，匹配前面不是abc的位置',
    example: '(?<!\\$)\\d+',
    matches: ['100', '50'],
    category: '分组和引用',
  },

  // 位置
  {
    pattern: '^',
    description: '匹配行的开始',
    example: '^Hello',
    matches: ['Hello World'],
    category: '位置',
  },
  {
    pattern: '$',
    description: '匹配行的结束',
    example: 'World$',
    matches: ['Hello World'],
    category: '位置',
  },

  // 常用模式
  {
    pattern: '^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$',
    description: '邮箱地址',
    example: 'user@example.com',
    matches: ['user@example.com', 'test.email@domain.co.uk'],
    category: '常用模式',
  },
  {
    pattern: '^1[3-9]\\d{9}$',
    description: '中国大陆手机号',
    example: '13812345678',
    matches: ['13812345678', '15912345678'],
    category: '常用模式',
  },
  {
    pattern: '^(https?|ftp)://[^\\s/$.?#].[^\\s]*$',
    description: 'URL地址',
    example: 'https://www.example.com',
    matches: ['https://www.example.com', 'http://test.org/path'],
    category: '常用模式',
  },
  {
    pattern: '^\\d{4}[-/]\\d{1,2}[-/]\\d{1,2}$',
    description: '日期格式 (YYYY-MM-DD)',
    example: '2024-01-15',
    matches: ['2024-01-15', '2024/1/5'],
    category: '常用模式',
  },
  {
    pattern: '^\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}$',
    description: 'IPv4地址',
    example: '192.168.1.1',
    matches: ['192.168.1.1', '10.0.0.1'],
    category: '常用模式',
  },
  {
    pattern: '^#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{3})$',
    description: '十六进制颜色值',
    example: '#FF5733',
    matches: ['#FF5733', '#abc', '#ABCDEF'],
    category: '常用模式',
  },
  {
    pattern: '^[1-9]\\d{5}$',
    description: '中国邮政编码',
    example: '100000',
    matches: ['100000', '518000'],
    category: '常用模式',
  },
  {
    pattern: '^[1-9]\\d{7}((0\\d)|(1[0-2]))(([0|1|2]\\d)|3[0-1])\\d{3}$',
    description: '身份证号码（15位）',
    example: '110101850101123',
    matches: ['110101850101123'],
    category: '常用模式',
  },
  {
    pattern: '^[1-9]\\d{5}(18|19|20)\\d{2}((0[1-9])|(1[0-2]))(([0-2][1-9])|10|20|30|31)\\d{3}[0-9Xx]$',
    description: '身份证号码（18位）',
    example: '110101199003071234',
    matches: ['110101199003071234'],
    category: '常用模式',
  },
  {
    pattern: '^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$',
    description: 'IPv4地址（精确）',
    example: '192.168.1.1',
    matches: ['192.168.1.1', '255.255.255.0'],
    category: '常用模式',
  },
  {
    pattern: '^(?=.*[a-z])(?=.*[A-Z])(?=.*\\d)[a-zA-Z\\d]{8,}$',
    description: '密码强度（至少8位，包含大小写字母和数字）',
    example: 'Password123',
    matches: ['Password123', 'MyPass123'],
    category: '常用模式',
  },
  {
    pattern: '<([a-z]+)([^<]+)*(?:>(.*)<\\/\\1>|\\s+\\/>)',
    description: 'HTML标签',
    example: '<div>content</div>',
    matches: ['<div>content</div>', '<img src="test.jpg" />'],
    category: '常用模式',
  },
  {
    pattern: '\\{\\{.*?\\}\\}',
    description: '模板变量 {{...}}',
    example: '{{name}}',
    matches: ['{{name}}', '{{user.age}}'],
    category: '常用模式',
  },
  {
    pattern: '/\\*.*?\\*/',
    description: '多行注释 /* ... */',
    example: '/* comment */',
    matches: ['/* comment */', '/* multi\nline */'],
    category: '常用模式',
  },
  {
    pattern: '//.*$',
    description: '单行注释 // ...',
    example: '// comment',
    matches: ['// comment', 'code // inline'],
    category: '常用模式',
  },
]

export const regexCategories = [...new Set(regexPatterns.map(p => p.category))]

export const regexFlags = [
  { flag: 'g', description: '全局匹配，查找所有匹配项' },
  { flag: 'i', description: '忽略大小写' },
  { flag: 'm', description: '多行模式，^和$匹配每行的开始和结束' },
  { flag: 's', description: '点号匹配换行符' },
  { flag: 'u', description: 'Unicode模式' },
  { flag: 'y', description: '粘性模式，从lastIndex位置开始匹配' },
]
