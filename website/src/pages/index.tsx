import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import styles from './index.module.css';

export default function Home() {
  const {i18n} = useDocusaurusContext();
  const en = i18n.currentLocale === 'en';
  const copy = en ? {
    tagline: 'A tile-based 2D vector renderer for Rust and WGPU, with a simple immediate Canvas and a general retained scene for large applications.',
    start: 'Get started', architecture: 'Architecture',
    features: [
      ['Tile-based GPU pipeline', 'Path scan, coarse binning, and fine raster operate on 16×16 tiles across native and portable WGPU.'],
      ['General retained scenes', 'Transactional hierarchy, incremental chunks and arenas, tile pages, plan fragments, and output history.'],
      ['Complete drawing stack', 'Paths, SDFs, text, images, gradients, masks, filters, backdrops, SVG, debugging, and profiling.'],
    ],
  } : {
    tagline: '面向 Rust/WGPU 的 tile-based 2D vector renderer。既支持简单的 immediate Canvas，也支持大型 UI 的通用 retained scene。',
    start: '开始使用', architecture: '阅读架构',
    features: [
      ['Tile-based GPU pipeline', 'Path scan、coarse binning 与 fine raster 按 16×16 tile 工作，支持 native 与 portable WGPU。'],
      ['General retained scenes', '事务式层级、增量 chunk/arena、tile pages、plan fragments 与输出历史，按实际变化付费。'],
      ['Complete drawing stack', '路径、SDF、文本、图片、渐变、mask、filter、backdrop、SVG 与调试 profiler。'],
    ],
  };
  return <Layout title="Rust tile renderer" description="Tileink architecture, guides and public API">
    <main>
      <section className={styles.hero}>
        <Heading as="h1">Tileink</Heading>
        <p>{copy.tagline}</p>
        <div className={styles.actions}>
          <Link className={clsx('button','button--primary','button--lg')} to="/docs/getting-started/quick-start">{copy.start}</Link>
          <Link className={clsx('button','button--secondary','button--lg')} to="/docs/architecture/overview">{copy.architecture}</Link>
        </div>
      </section>
      <section className={styles.features}>
        {copy.features.map(([title, body]) => <article className={styles.card} key={title}><Heading as="h2">{title}</Heading><p>{body}</p></article>)}
      </section>
    </main>
  </Layout>;
}
