import { useEffect, useState, useCallback, useRef } from 'react';

//
// ASCII art for PRAXIS logo (praxis_dark theme).
//
const PRAXIS_LOGO = [
  '  ██████╗ ██████╗  █████╗ ██╗  ██╗██╗███████╗',
  '  ██╔══██╗██╔══██╗██╔══██╗╚██╗██╔╝██║██╔════╝',
  '  ██████╔╝██████╔╝███████║ ╚███╔╝ ██║███████╗',
  '  ██╔═══╝ ██╔══██╗██╔══██║ ██╔██╗ ██║╚════██║',
  '  ██║     ██║  ██║██║  ██║██╔╝ ██╗██║███████║',
  '  ╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝',
];

const SUBTITLE = 'by [Ø] Origin';

//
// Glitch characters for the matrix effect.
//
const GLITCH_CHARS = [
  '░', '▒', '▓', '█', '▄', '▀', '■', '□', '▪', '▫',
  '╔', '╗', '╚', '╝', '═', '║', '╬', '╣', '╠', '╩', '╦',
  '0', '1', '@', '#', '$', '%', '&', '*', '!', '?',
  'Ø', 'Δ', 'Σ', 'Π', 'λ', 'μ', 'ψ', 'Ω', 'φ', 'θ',
];

interface SplashScreenProps {
  onComplete: () => void;
}

interface MatrixChar {
  x: number;
  y: number;
  char: string;
  intensity: number;
}

//
// Detect if dark theme is active by checking CSS variable.
//
function useThemeDetection(): 'dark' | 'light' {
  const [theme, setTheme] = useState<'dark' | 'light'>('light');

  useEffect(() => {
    const isDark = getComputedStyle(document.documentElement)
      .getPropertyValue('--theme-is-dark')
      .trim() === '1';
    setTheme(isDark ? 'dark' : 'light');
  }, []);

  return theme;
}

//
// Origin Light splash screen - ASCII art with ink/paper effects.
//
function OriginLightSplash({ onComplete }: SplashScreenProps) {
  const [phase, setPhase] = useState(0);
  const [progress, setProgress] = useState(0);
  const [particles, setParticles] = useState<MatrixChar[]>([]);
  const [scanlineY, setScanlineY] = useState(0);
  const [logoCharsVisible, setLogoCharsVisible] = useState(0);
  const [subtitleVisible, setSubtitleVisible] = useState(0);
  const [exitProgress, setExitProgress] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const startTimeRef = useRef(Date.now());
  const animationFrameRef = useRef<number | undefined>(undefined);

  const totalDuration = 3000;
  const phase1End = 800;
  const phase2End = 2200;

  const totalLogoChars = PRAXIS_LOGO.reduce((sum, line) => sum + line.length, 0);

  const getRandomGlitchChar = useCallback(() => {
    return GLITCH_CHARS[Math.floor(Math.random() * GLITCH_CHARS.length)];
  }, []);

  //
  // Generate floating particles with muted tones.
  //
  const generateParticles = useCallback((density: number) => {
    const chars: MatrixChar[] = [];
    const cols = Math.floor(window.innerWidth / 10);
    const rows = Math.floor(window.innerHeight / 16);

    for (let i = 0; i < cols * rows * density; i++) {
      chars.push({
        x: Math.random() * 100,
        y: Math.random() * 100,
        char: getRandomGlitchChar(),
        intensity: Math.floor(Math.random() * 60) + 140,
      });
    }
    return chars;
  }, [getRandomGlitchChar]);

  useEffect(() => {
    const animate = () => {
      const elapsed = Date.now() - startTimeRef.current;
      const overallProgress = Math.min(elapsed / totalDuration, 1);
      setProgress(overallProgress);

      if (elapsed < phase1End) {
        setPhase(0);
        const phaseProgress = elapsed / phase1End;
        setParticles(generateParticles(phaseProgress * 0.25));
        setLogoCharsVisible(Math.floor(totalLogoChars * phaseProgress));
        if (phaseProgress > 0.7) {
          setSubtitleVisible(Math.floor(SUBTITLE.length * ((phaseProgress - 0.7) / 0.3)));
        }
      } else if (elapsed < phase2End) {
        setPhase(1);
        setLogoCharsVisible(totalLogoChars);
        setSubtitleVisible(SUBTITLE.length);
        setParticles(generateParticles(0.015));
        setScanlineY((prev) => (prev + 1.5) % 100);
      } else if (elapsed < totalDuration) {
        setPhase(2);
        const exitProg = (elapsed - phase2End) / (totalDuration - phase2End);
        setExitProgress(exitProg);
      } else {
        onComplete();
        return;
      }

      animationFrameRef.current = requestAnimationFrame(animate);
    };

    animationFrameRef.current = requestAnimationFrame(animate);

    return () => {
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
      }
    };
  }, [generateParticles, onComplete, totalLogoChars]);

  useEffect(() => {
    const handleKeyPress = (e: KeyboardEvent) => {
      e.preventDefault();
      onComplete();
    };

    const handleClick = () => {
      onComplete();
    };

    window.addEventListener('keydown', handleKeyPress);
    window.addEventListener('click', handleClick);

    return () => {
      window.removeEventListener('keydown', handleKeyPress);
      window.removeEventListener('click', handleClick);
    };
  }, [onComplete]);

  const pulse = phase === 1 ? Math.sin(progress * Math.PI * 6) * 0.08 + 0.92 : 1;

  const renderLogoChar = (char: string, charIndex: number, lineIndex: number) => {
    const globalIndex = PRAXIS_LOGO.slice(0, lineIndex).reduce((sum, l) => sum + l.length, 0) + charIndex;

    if (phase === 2) {
      const visibility = 1 - exitProgress;
      if (Math.random() > visibility) {
        return <span key={charIndex} style={{ opacity: 0 }}>{char}</span>;
      }
      const intensity = 1 - exitProgress;
      return (
        <span
          key={charIndex}
          style={{
            color: `rgba(24, 22, 18, ${intensity})`,
            textShadow: exitProgress < 0.5 ? `0 0 ${8 - exitProgress * 16}px rgba(24, 22, 18, 0.2)` : 'none',
          }}
        >
          {Math.random() < exitProgress * 0.5 ? getRandomGlitchChar() : char}
        </span>
      );
    }

    if (globalIndex < logoCharsVisible) {
      const gradientFactor = 1 - (charIndex / PRAXIS_LOGO[0].length) * 0.15;
      const alpha = pulse * gradientFactor;
      const showGlitch = phase === 0 && Math.random() < 0.05;

      return (
        <span
          key={charIndex}
          style={{
            color: `rgba(24, 22, 18, ${alpha})`,
            textShadow: '0 1px 2px rgba(24, 22, 18, 0.1)',
          }}
        >
          {showGlitch ? getRandomGlitchChar() : char}
        </span>
      );
    } else if (globalIndex < logoCharsVisible + 5 && phase === 0) {
      return (
        <span
          key={charIndex}
          style={{ color: 'rgba(120, 119, 110, 0.6)' }}
        >
          {getRandomGlitchChar()}
        </span>
      );
    }

    return <span key={charIndex} style={{ opacity: 0 }}>{char}</span>;
  };

  return (
    <div
      ref={containerRef}
      className="fixed inset-0 z-50 flex items-center justify-center overflow-hidden"
      style={{
        backgroundColor: '#d8d6d1',
        cursor: 'pointer',
      }}
    >
      {/* Noise texture - matches main pane but more prominent */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          backgroundImage: `url("data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%20width%3D%22140%22%20height%3D%22140%22%20viewBox%3D%220%200%20140%20140%22%3E%3Cfilter%20id%3D%22n%22%20x%3D%220%22%20y%3D%220%22%20width%3D%22100%25%22%20height%3D%22100%25%22%3E%3CfeTurbulence%20type%3D%22fractalNoise%22%20baseFrequency%3D%220.9%22%20numOctaves%3D%224%22/%3E%3C/filter%3E%3Crect%20width%3D%22100%25%22%20height%3D%22100%25%22%20filter%3D%22url%28%23n%29%22%20opacity%3D%221%22/%3E%3C/svg%3E")`,
          backgroundRepeat: 'repeat',
          backgroundSize: '140px 140px',
          opacity: 0.45,
          filter: 'brightness(0.5) contrast(0.25)',
          mixBlendMode: 'multiply' as const,
        }}
      />

      {/* Floating particles */}
      <div className="absolute inset-0 pointer-events-none overflow-hidden">
        {particles.map((p, i) => (
          <span
            key={i}
            className="absolute font-mono text-xs"
            style={{
              left: `${p.x}%`,
              top: `${p.y}%`,
              color: `rgba(120, 119, 110, ${(255 - p.intensity) / 255 * 0.4})`,
              opacity: phase === 2 ? 1 - exitProgress : 1,
            }}
          >
            {p.char}
          </span>
        ))}
      </div>

      {/* Subtle scanline */}
      {phase === 1 && (
        <div
          className="absolute left-0 right-0 h-px pointer-events-none"
          style={{
            top: `${scanlineY}%`,
            backgroundColor: 'rgba(24, 22, 18, 0.08)',
            boxShadow: '0 0 8px rgba(24, 22, 18, 0.05)',
          }}
        />
      )}

      {/* Logo container */}
      <div className="text-center select-none" style={{ opacity: phase === 2 ? 1 - exitProgress * 0.5 : 1 }}>
        <pre
          className="font-mono text-sm md:text-base lg:text-lg leading-tight"
          style={{ fontFamily: 'monospace' }}
        >
          {PRAXIS_LOGO.map((line, lineIndex) => (
            <div key={lineIndex}>
              {line.split('').map((char, charIndex) => renderLogoChar(char, charIndex, lineIndex))}
            </div>
          ))}
        </pre>

        {/* Subtitle */}
        <div
          className="mt-4 text-sm italic tracking-wider"
          style={{
            color: `rgba(92, 91, 84, ${pulse})`,
            opacity: phase === 2 ? Math.max(0, (1 - exitProgress - 0.3) / 0.7) : 1,
          }}
        >
          {SUBTITLE.slice(0, subtitleVisible)}
          {subtitleVisible < SUBTITLE.length && phase === 0 && (
            <span className="animate-pulse">_</span>
          )}
        </div>

        {/* Press any key hint */}
        <div
          className="mt-8 text-xs tracking-widest uppercase"
          style={{
            color: 'rgba(92, 91, 84, 0.6)',
            opacity: phase === 1 ? 0.6 + Math.sin(Date.now() / 500) * 0.2 : 0,
          }}
        >
          Press any key to continue
        </div>
      </div>

    </div>
  );
}

//
// Praxis Dark splash screen - ASCII art with matrix effects.
//
function PraxisDarkSplash({ onComplete }: SplashScreenProps) {
  //
  // 0: matrix + reveal, 1: stable, 2: exit.
  //
  const [phase, setPhase] = useState(0);
  const [progress, setProgress] = useState(0);
  const [matrixChars, setMatrixChars] = useState<MatrixChar[]>([]);
  const [scanlineY, setScanlineY] = useState(0);
  const [logoCharsVisible, setLogoCharsVisible] = useState(0);
  const [subtitleVisible, setSubtitleVisible] = useState(0);
  const [exitProgress, setExitProgress] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const startTimeRef = useRef(Date.now());
  const animationFrameRef = useRef<number | undefined>(undefined);

  const totalDuration = 3000;
  const phase1End = 800;
  const phase2End = 2200;

  //
  // Total characters in logo.
  //
  const totalLogoChars = PRAXIS_LOGO.reduce((sum, line) => sum + line.length, 0);

  //
  // Generate random glitch char.
  //
  const getRandomGlitchChar = useCallback(() => {
    return GLITCH_CHARS[Math.floor(Math.random() * GLITCH_CHARS.length)];
  }, []);

  //
  // Generate matrix rain characters.
  //
  const generateMatrixChars = useCallback((density: number) => {
    const chars: MatrixChar[] = [];
    const cols = Math.floor(window.innerWidth / 10);
    const rows = Math.floor(window.innerHeight / 16);

    for (let i = 0; i < cols * rows * density; i++) {
      chars.push({
        x: Math.random() * 100,
        y: Math.random() * 100,
        char: getRandomGlitchChar(),
        intensity: Math.floor(Math.random() * 60) + 20,
      });
    }
    return chars;
  }, [getRandomGlitchChar]);

  //
  // Animation loop.
  //
  useEffect(() => {
    const animate = () => {
      const elapsed = Date.now() - startTimeRef.current;
      const overallProgress = Math.min(elapsed / totalDuration, 1);
      setProgress(overallProgress);

      if (elapsed < phase1End) {
        //
        // Phase 1: Matrix rain + logo reveal.
        //
        setPhase(0);
        const phaseProgress = elapsed / phase1End;
        setMatrixChars(generateMatrixChars(phaseProgress * 0.3));
        setLogoCharsVisible(Math.floor(totalLogoChars * phaseProgress));
        if (phaseProgress > 0.7) {
          setSubtitleVisible(Math.floor(SUBTITLE.length * ((phaseProgress - 0.7) / 0.3)));
        }
      } else if (elapsed < phase2End) {
        //
        // Phase 2: Stable display.
        //
        setPhase(1);
        setLogoCharsVisible(totalLogoChars);
        setSubtitleVisible(SUBTITLE.length);
        //
        // Sparse particles.
        //
        setMatrixChars(generateMatrixChars(0.02));
        setScanlineY((prev) => (prev + 2) % 100);
      } else if (elapsed < totalDuration) {
        //
        // Phase 3: Exit animation.
        //
        setPhase(2);
        const exitProg = (elapsed - phase2End) / (totalDuration - phase2End);
        setExitProgress(exitProg);
      } else {
        //
        // Animation complete.
        //
        onComplete();
        return;
      }

      animationFrameRef.current = requestAnimationFrame(animate);
    };

    animationFrameRef.current = requestAnimationFrame(animate);

    return () => {
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
      }
    };
  }, [generateMatrixChars, onComplete, totalLogoChars]);

  //
  // Handle keypress or click to skip.
  //
  useEffect(() => {
    const handleKeyPress = (e: KeyboardEvent) => {
      e.preventDefault();
      onComplete();
    };

    const handleClick = () => {
      onComplete();
    };

    window.addEventListener('keydown', handleKeyPress);
    window.addEventListener('click', handleClick);

    return () => {
      window.removeEventListener('keydown', handleKeyPress);
      window.removeEventListener('click', handleClick);
    };
  }, [onComplete]);

  //
  // Calculate pulse effect for stable phase.
  //
  const pulse = phase === 1 ? Math.sin(progress * Math.PI * 6) * 0.15 + 0.85 : 1;

  //
  // Render logo character with effects.
  //
  const renderLogoChar = (char: string, charIndex: number, lineIndex: number) => {
    const globalIndex = PRAXIS_LOGO.slice(0, lineIndex).reduce((sum, l) => sum + l.length, 0) + charIndex;

    if (phase === 2) {
      //
      // Exit animation - random disappearance.
      //
      const visibility = 1 - exitProgress;
      if (Math.random() > visibility) {
        return <span key={charIndex} style={{ opacity: 0 }}>{char}</span>;
      }
      const intensity = 1 - exitProgress;
      return (
        <span
          key={charIndex}
          style={{
            color: `rgb(${Math.floor(242 * intensity)}, ${Math.floor(255 * intensity)}, ${Math.floor(213 * intensity)})`,
            textShadow: exitProgress < 0.5 ? `0 0 ${15 - exitProgress * 30}px rgba(158, 230, 117, 0.5)` : 'none',
          }}
        >
          {Math.random() < exitProgress * 0.5 ? getRandomGlitchChar() : char}
        </span>
      );
    }

    if (globalIndex < logoCharsVisible) {
      //
      // Revealed character - use highlight color (#f2ffd5) with green glow.
      //
      const gradientFactor = 1 - (charIndex / PRAXIS_LOGO[0].length) * 0.2;
      const r = Math.floor(242 * pulse * gradientFactor);
      const g = Math.floor(255 * pulse * gradientFactor);
      const b = Math.floor(213 * pulse * gradientFactor);

      //
      // Occasional glitch during reveal.
      //
      const showGlitch = phase === 0 && Math.random() < 0.05;

      return (
        <span
          key={charIndex}
          style={{
            color: `rgb(${r}, ${g}, ${b})`,
            textShadow: `0 0 15px rgba(158, 230, 117, 0.6), 0 0 30px rgba(158, 230, 117, 0.3)`,
          }}
        >
          {showGlitch ? getRandomGlitchChar() : char}
        </span>
      );
    } else if (globalIndex < logoCharsVisible + 5 && phase === 0) {
      //
      // Leading edge glitch - use accent green.
      //
      return (
        <span
          key={charIndex}
          style={{ color: '#5c9c66' }}
        >
          {getRandomGlitchChar()}
        </span>
      );
    }

    return <span key={charIndex} style={{ opacity: 0 }}>{char}</span>;
  };

  return (
    <div
      ref={containerRef}
      className="fixed inset-0 z-50 flex items-center justify-center overflow-hidden"
      style={{
        backgroundColor: '#030712',
        cursor: 'pointer',
      }}
    >
      {/* Noise texture - adds depth to the background */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          backgroundImage: `url("data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%20width%3D%22140%22%20height%3D%22140%22%20viewBox%3D%220%200%20140%20140%22%3E%3Cfilter%20id%3D%22n%22%20x%3D%220%22%20y%3D%220%22%20width%3D%22100%25%22%20height%3D%22100%25%22%3E%3CfeTurbulence%20type%3D%22fractalNoise%22%20baseFrequency%3D%220.9%22%20numOctaves%3D%224%22/%3E%3C/filter%3E%3Crect%20width%3D%22100%25%22%20height%3D%22100%25%22%20filter%3D%22url%28%23n%29%22%20opacity%3D%221%22/%3E%3C/svg%3E")`,
          backgroundRepeat: 'repeat',
          backgroundSize: '140px 140px',
          opacity: 0.15,
          filter: 'brightness(0.3) contrast(0.4)',
          mixBlendMode: 'screen' as const,
        }}
      />

      {/*
      //
      // Matrix rain / particles background.
      //
      */}
      <div className="absolute inset-0 pointer-events-none overflow-hidden">
        {matrixChars.map((mc, i) => (
          <span
            key={i}
            className="absolute font-mono text-xs"
            style={{
              left: `${mc.x}%`,
              top: `${mc.y}%`,
              color: `rgb(${Math.floor(mc.intensity * 0.4)}, ${mc.intensity}, ${Math.floor(mc.intensity * 0.5)})`,
              opacity: phase === 2 ? 1 - exitProgress : 1,
            }}
          >
            {mc.char}
          </span>
        ))}
      </div>

      {/*
      //
      // Scanline effect.
      //
      */}
      {phase === 1 && (
        <div
          className="absolute left-0 right-0 h-px pointer-events-none"
          style={{
            top: `${scanlineY}%`,
            backgroundColor: 'rgba(158, 230, 117, 0.3)',
            boxShadow: '0 0 10px rgba(158, 230, 117, 0.2)',
          }}
        />
      )}

      {/*
      //
      // Logo container.
      //
      */}
      <div className="text-center select-none" style={{ opacity: phase === 2 ? 1 - exitProgress * 0.5 : 1 }}>
        {/*
        //
        // PRAXIS Logo.
        //
        */}
        <pre
          className="font-mono text-sm md:text-base lg:text-lg leading-tight"
          style={{ fontFamily: 'monospace' }}
        >
          {PRAXIS_LOGO.map((line, lineIndex) => (
            <div key={lineIndex}>
              {line.split('').map((char, charIndex) => renderLogoChar(char, charIndex, lineIndex))}
            </div>
          ))}
        </pre>

        {/*
        //
        // Subtitle.
        //
        */}
        <div
          className="mt-4 text-sm italic tracking-wider"
          style={{
            color: `rgb(${Math.floor(92 * pulse)}, ${Math.floor(156 * pulse)}, ${Math.floor(102 * pulse)})`,
            opacity: phase === 2 ? Math.max(0, (1 - exitProgress - 0.3) / 0.7) : 1,
          }}
        >
          {SUBTITLE.slice(0, subtitleVisible)}
          {subtitleVisible < SUBTITLE.length && phase === 0 && (
            <span className="animate-pulse">_</span>
          )}
        </div>

        {/*
        //
        // Press any key hint.
        //
        */}
        <div
          className="mt-8 text-xs tracking-widest uppercase"
          style={{
            color: 'rgb(92, 156, 102)',
            opacity: phase === 1 ? 0.6 + Math.sin(Date.now() / 500) * 0.2 : 0,
          }}
        >
          Press any key to continue
        </div>
      </div>

      {/* Subtle grid overlay - matches main pane */}
      <div
        className="absolute inset-0 pointer-events-none"
        style={{
          backgroundImage:
            'linear-gradient(rgba(31, 50, 41, 0.2) 1px, transparent 1px), linear-gradient(90deg, rgba(31, 50, 41, 0.2) 1px, transparent 1px)',
          backgroundSize: '3px 3px',
          opacity: 0.3,
        }}
      />
    </div>
  );
}

//
// Main splash screen component that delegates to theme-specific variant.
//
export function SplashScreen({ onComplete }: SplashScreenProps) {
  const theme = useThemeDetection();

  if (theme === 'dark') {
    return <PraxisDarkSplash onComplete={onComplete} />;
  }

  return <OriginLightSplash onComplete={onComplete} />;
}
