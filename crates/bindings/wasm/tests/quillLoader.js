import * as fs from 'fs';
import * as path from 'path';

function loadDirectory(dirPath) {
  const result = {};
  const entries = fs.readdirSync(dirPath, { withFileTypes: true });

  for (const entry of entries) {
    const fullPath = path.join(dirPath, entry.name);

    if (entry.isDirectory()) {
      result[entry.name] = loadDirectory(fullPath);
    } else if (entry.isFile()) {
      const isBinary = /\.(png|jpg|jpeg|gif|pdf|woff|woff2|ttf|otf)$/i.test(entry.name);

      if (isBinary) {
        const buffer = fs.readFileSync(fullPath);
        result[entry.name] = {
          contents: Array.from(buffer)
        };
      } else {
        const text = fs.readFileSync(fullPath, 'utf8');
        result[entry.name] = {
          contents: text
        };
      }
    }
  }

  return result;
}

export function loadQuill(quillPath) {
  const files = loadDirectory(quillPath);

  return {
    files: files
  };
}
