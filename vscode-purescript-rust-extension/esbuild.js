const esbuild = require('esbuild');
const path = require('path');

const build = async () => {
  try {
    await esbuild.build({
      entryPoints: ['src/extension.ts'],
      bundle: true,
      platform: 'node',
      format: 'cjs',
      outfile: 'dist/extension.js',
      external: ['vscode'], // vscode is provided by the extension host
      sourcemap: true,
      minify: false, // Keep readable for debugging
      target: 'node14', // Match VS Code's Node.js version
      define: {
        'process.env.NODE_ENV': '"production"'
      }
    });
    console.log('✅ Extension bundled successfully');
  } catch (error) {
    console.error('❌ Build failed:', error);
    process.exit(1);
  }
};

build();
