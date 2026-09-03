export interface JsMethod {
  name: string
  description: string
  syntax: string
  example: string
  category: string
}

export const jsMethods: JsMethod[] = [
  // 数组方法
  {
    name: 'Array.map()',
    description: '创建一个新数组，其中每个元素是调用一次提供的函数后的返回值',
    syntax: 'array.map(callback(currentValue, index, array))',
    example: 'const doubled = [1, 2, 3].map(x => x * 2); // [2, 4, 6]',
    category: '数组',
  },
  {
    name: 'Array.filter()',
    description: '创建一个新数组，包含通过所提供函数实现的测试的所有元素',
    syntax: 'array.filter(callback(element, index, array))',
    example: 'const evens = [1, 2, 3, 4].filter(x => x % 2 === 0); // [2, 4]',
    category: '数组',
  },
  {
    name: 'Array.reduce()',
    description: '对数组中的每个元素执行一个提供的 reducer 函数，将其结果汇总为单个返回值',
    syntax: 'array.reduce(callback(accumulator, currentValue, index, array), initialValue)',
    example: 'const sum = [1, 2, 3].reduce((acc, val) => acc + val, 0); // 6',
    category: '数组',
  },
  {
    name: 'Array.forEach()',
    description: '对数组的每个元素执行一次提供的函数',
    syntax: 'array.forEach(callback(currentValue, index, array))',
    example: '[1, 2, 3].forEach(x => console.log(x));',
    category: '数组',
  },
  {
    name: 'Array.find()',
    description: '返回数组中满足提供的测试函数的第一个元素的值',
    syntax: 'array.find(callback(element, index, array))',
    example: 'const found = [1, 2, 3].find(x => x > 1); // 2',
    category: '数组',
  },
  {
    name: 'Array.findIndex()',
    description: '返回数组中满足提供的测试函数的第一个元素的索引',
    syntax: 'array.findIndex(callback(element, index, array))',
    example: 'const index = [1, 2, 3].findIndex(x => x > 1); // 1',
    category: '数组',
  },
  {
    name: 'Array.some()',
    description: '测试数组中是不是至少有1个元素通过了被提供的函数测试',
    syntax: 'array.some(callback(element, index, array))',
    example: 'const hasEven = [1, 2, 3].some(x => x % 2 === 0); // true',
    category: '数组',
  },
  {
    name: 'Array.every()',
    description: '测试一个数组内的所有元素是否都能通过某个指定函数的测试',
    syntax: 'array.every(callback(element, index, array))',
    example: 'const allEven = [2, 4, 6].every(x => x % 2 === 0); // true',
    category: '数组',
  },
  {
    name: 'Array.includes()',
    description: '判断一个数组是否包含一个指定的值',
    syntax: 'array.includes(searchElement, fromIndex)',
    example: '[1, 2, 3].includes(2); // true',
    category: '数组',
  },
  {
    name: 'Array.flat()',
    description: '按照一个可指定的深度递归遍历数组，并将所有元素与遍历到的子数组中的元素合并为一个新数组返回',
    syntax: 'array.flat(depth)',
    example: '[1, [2, [3]]].flat(Infinity); // [1, 2, 3]',
    category: '数组',
  },
  {
    name: 'Array.flatMap()',
    description: '首先使用映射函数映射每个元素，然后将结果压缩成一个新数组',
    syntax: 'array.flatMap(callback(currentValue, index, array))',
    example: '[1, 2, 3].flatMap(x => [x, x * 2]); // [1, 2, 2, 4, 3, 6]',
    category: '数组',
  },
  {
    name: 'Array.sort()',
    description: '对数组的元素进行排序',
    syntax: 'array.sort(compareFunction)',
    example: '[3, 1, 2].sort((a, b) => a - b); // [1, 2, 3]',
    category: '数组',
  },
  {
    name: 'Array.splice()',
    description: '通过删除或替换现有元素或者原地添加新的元素来修改数组',
    syntax: 'array.splice(start, deleteCount, item1, item2, ...)',
    example: 'const arr = [1, 2, 3]; arr.splice(1, 1); // arr = [1, 3]',
    category: '数组',
  },
  {
    name: 'Array.slice()',
    description: '返回一个新的数组对象，这一对象是一个由 begin 和 end 决定的原数组的浅拷贝',
    syntax: 'array.slice(begin, end)',
    example: '[1, 2, 3].slice(1, 2); // [2]',
    category: '数组',
  },

  // 字符串方法
  {
    name: 'String.split()',
    description: '使用指定的分隔符字符串将一个 String 对象分割成子字符串数组',
    syntax: 'string.split(separator, limit)',
    example: '"hello world".split(" "); // ["hello", "world"]',
    category: '字符串',
  },
  {
    name: 'String.join()',
    description: '将一个数组（或一个类数组对象）的所有元素连接成一个字符串',
    syntax: 'array.join(separator)',
    example: '["hello", "world"].join(" "); // "hello world"',
    category: '字符串',
  },
  {
    name: 'String.replace()',
    description: '返回一个由替换值替换部分或所有的模式匹配项后的新字符串',
    syntax: 'string.replace(regexp|substr, newSubStr|function)',
    example: '"hello".replace("l", "L"); // "heLlo"',
    category: '字符串',
  },
  {
    name: 'String.replaceAll()',
    description: '返回一个新字符串，其中所有满足 pattern 的部分都已被 replacement 替换',
    syntax: 'string.replaceAll(regexp|substr, newSubStr|function)',
    example: '"aabbcc".replaceAll("b", "x"); // "aaxxcc"',
    category: '字符串',
  },
  {
    name: 'String.trim()',
    description: '从字符串的两端清除空格',
    syntax: 'string.trim()',
    example: '"  hello  ".trim(); // "hello"',
    category: '字符串',
  },
  {
    name: 'String.startsWith()',
    description: '判断当前字符串是否以另外一个给定的子字符串开头',
    syntax: 'string.startsWith(searchString, position)',
    example: '"hello".startsWith("he"); // true',
    category: '字符串',
  },
  {
    name: 'String.endsWith()',
    description: '判断当前字符串是否以另外一个给定的子字符串结尾',
    syntax: 'string.endsWith(searchString, length)',
    example: '"hello".endsWith("lo"); // true',
    category: '字符串',
  },
  {
    name: 'String.includes()',
    description: '判断一个字符串是否包含在另一个字符串中',
    syntax: 'string.includes(searchString, position)',
    example: '"hello".includes("ell"); // true',
    category: '字符串',
  },
  {
    name: 'String.padStart()',
    description: '用另一个字符串从当前字符串的开头重复填充',
    syntax: 'string.padStart(targetLength, padString)',
    example: '"5".padStart(3, "0"); // "005"',
    category: '字符串',
  },
  {
    name: 'String.padEnd()',
    description: '用另一个字符串从当前字符串的末尾重复填充',
    syntax: 'string.padEnd(targetLength, padString)',
    example: '"5".padEnd(3, "0"); // "500"',
    category: '字符串',
  },

  // 对象方法
  {
    name: 'Object.keys()',
    description: '返回一个由一个给定对象的自身可枚举属性名组成的数组',
    syntax: 'Object.keys(obj)',
    example: 'Object.keys({a: 1, b: 2}); // ["a", "b"]',
    category: '对象',
  },
  {
    name: 'Object.values()',
    description: '返回一个给定对象自身的所有可枚举属性值的数组',
    syntax: 'Object.values(obj)',
    example: 'Object.values({a: 1, b: 2}); // [1, 2]',
    category: '对象',
  },
  {
    name: 'Object.entries()',
    description: '返回一个给定对象自身可枚举属性的键值对数组',
    syntax: 'Object.entries(obj)',
    example: 'Object.entries({a: 1, b: 2}); // [["a", 1], ["b", 2]]',
    category: '对象',
  },
  {
    name: 'Object.assign()',
    description: '将所有可枚举自有属性的值从一个或多个源对象复制到目标对象',
    syntax: 'Object.assign(target, ...sources)',
    example: 'Object.assign({}, {a: 1}, {b: 2}); // {a: 1, b: 2}',
    category: '对象',
  },
  {
    name: 'Object.freeze()',
    description: '冻结一个对象，使其不能被修改',
    syntax: 'Object.freeze(obj)',
    example: 'const obj = Object.freeze({a: 1});',
    category: '对象',
  },
  {
    name: 'Object.fromEntries()',
    description: '把键值对列表转换为一个对象',
    syntax: 'Object.fromEntries(iterable)',
    example: 'Object.fromEntries([["a", 1], ["b", 2]]); // {a: 1, b: 2}',
    category: '对象',
  },

  // Promise 方法
  {
    name: 'Promise.all()',
    description: '等待所有 promise 完成，或第一个失败',
    syntax: 'Promise.all(iterable)',
    example: 'await Promise.all([fetch("/a"), fetch("/b")]);',
    category: 'Promise',
  },
  {
    name: 'Promise.allSettled()',
    description: '等待所有 promise 完成，无论成功或失败',
    syntax: 'Promise.allSettled(iterable)',
    example: 'await Promise.allSettled([p1, p2]);',
    category: 'Promise',
  },
  {
    name: 'Promise.race()',
    description: '返回第一个完成的 promise 的结果',
    syntax: 'Promise.race(iterable)',
    example: 'await Promise.race([p1, p2]);',
    category: 'Promise',
  },
  {
    name: 'Promise.any()',
    description: '返回第一个成功的 promise 的结果',
    syntax: 'Promise.any(iterable)',
    example: 'await Promise.any([p1, p2]);',
    category: 'Promise',
  },

  // Map 和 Set
  {
    name: 'Map.set()',
    description: '为 Map 对象添加或更新一个指定了键（key）和值（value）的键值对',
    syntax: 'map.set(key, value)',
    example: 'const map = new Map(); map.set("key", "value");',
    category: 'Map & Set',
  },
  {
    name: 'Map.get()',
    description: '返回某个 Map 对象中的一个指定元素',
    syntax: 'map.get(key)',
    example: 'map.get("key"); // "value"',
    category: 'Map & Set',
  },
  {
    name: 'Map.has()',
    description: '返回一个布尔值，用于指示具有指定键的元素是否存在',
    syntax: 'map.has(key)',
    example: 'map.has("key"); // true',
    category: 'Map & Set',
  },
  {
    name: 'Set.add()',
    description: '在 Set 对象尾部添加一个元素',
    syntax: 'set.add(value)',
    example: 'const set = new Set(); set.add(1);',
    category: 'Map & Set',
  },
  {
    name: 'Set.has()',
    description: '返回一个布尔值，表示该值在 Set 中存在与否',
    syntax: 'set.has(value)',
    example: 'set.has(1); // true',
    category: 'Map & Set',
  },

  // JSON
  {
    name: 'JSON.parse()',
    description: '解析 JSON 字符串',
    syntax: 'JSON.parse(text, reviver)',
    example: 'JSON.parse(\'{"a": 1}\'); // {a: 1}',
    category: 'JSON',
  },
  {
    name: 'JSON.stringify()',
    description: '将 JavaScript 值转换为 JSON 字符串',
    syntax: 'JSON.stringify(value, replacer, space)',
    example: 'JSON.stringify({a: 1}); // \'{"a":1}\'',
    category: 'JSON',
  },

  // 异步
  {
    name: 'async/await',
    description: '异步函数语法糖，使异步代码更易读',
    syntax: 'async function name() { await promise; }',
    example: 'async function fetchData() { const res = await fetch("/api"); }',
    category: '异步',
  },
  {
    name: 'for await...of',
    description: '遍历异步可迭代对象',
    syntax: 'for await (variable of iterable) { }',
    example: 'for await (const chunk of readableStream) { }',
    category: '异步',
  },

  // ES6+ 语法
  {
    name: '解构赋值',
    description: '从数组或对象中提取值并赋给变量',
    syntax: 'const { a, b } = obj; const [x, y] = arr;',
    example: 'const { name, age } = { name: "John", age: 30 };',
    category: 'ES6+',
  },
  {
    name: '展开运算符',
    description: '展开数组或对象',
    syntax: '...iterable / ...obj',
    example: 'const arr = [...arr1, ...arr2]; const obj = {...obj1, ...obj2};',
    category: 'ES6+',
  },
  {
    name: '可选链',
    description: '安全地访问嵌套对象属性',
    syntax: 'obj?.prop / obj?.[expr] / obj?.method()',
    example: 'const city = user?.address?.city;',
    category: 'ES6+',
  },
  {
    name: '空值合并',
    description: '当左侧为 null 或 undefined 时返回右侧值',
    syntax: 'value ?? defaultValue',
    example: 'const name = user.name ?? "Anonymous";',
    category: 'ES6+',
  },
  {
    name: '模板字符串',
    description: '使用反引号创建字符串，支持嵌入表达式',
    syntax: '`string ${expression}`',
    example: 'const msg = `Hello, ${name}!`;',
    category: 'ES6+',
  },
  {
    name: '箭头函数',
    description: '更简洁的函数语法',
    syntax: '(params) => expression',
    example: 'const add = (a, b) => a + b;',
    category: 'ES6+',
  },
  {
    name: '类',
    description: 'ES6 类语法',
    syntax: 'class Name { constructor() {} method() {} }',
    example: 'class Person { constructor(name) { this.name = name; } }',
    category: 'ES6+',
  },
  {
    name: '模块导入导出',
    description: 'ES6 模块语法',
    syntax: 'import { x } from "./module"; export const y = 1;',
    example: 'import { useState } from "react";',
    category: 'ES6+',
  },
]

export const jsCategories = [...new Set(jsMethods.map(m => m.category))]
