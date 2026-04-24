// Precise animations and visual refinement
document.addEventListener('DOMContentLoaded', () => {
    // Hero Logo Animation
    const heroLogo = document.querySelector('.hero-logo');
    if (heroLogo) {
        let angle = 0;
        const animate = () => {
            angle += 0.02;
            const yOffset = Math.sin(angle) * 15;
            const rotate = Math.cos(angle * 0.5) * 2;
            heroLogo.style.transform = `translateY(${yOffset}px) rotate(${rotate}deg)`;
            requestAnimationFrame(animate);
        };
        animate();
    }

    // Smooth scrolling
    document.querySelectorAll('a[href^="#"]').forEach(anchor => {
        anchor.addEventListener('click', function (e) {
            const targetId = this.getAttribute('href');
            if (targetId === '#') return;

            e.preventDefault();
            const target = document.querySelector(targetId);
            if (target) {
                target.scrollIntoView({
                    behavior: 'smooth'
                });
            }
        });
    });

    // Reveal elements on scroll
    const observerOptions = {
        threshold: 0.15,
        rootMargin: '0px 0px -50px 0px'
    };

    const revealObserver = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                entry.target.classList.add('revealed');
                revealObserver.unobserve(entry.target);
            }
        });
    }, observerOptions);

    const elementsToReveal = document.querySelectorAll('.feature-card, .nuze-text, .nuze-visual, .hero-content');
    elementsToReveal.forEach(el => {
        el.style.opacity = '0';
        el.style.transform = 'translateY(30px)';
        el.style.transition = 'opacity 0.8s cubic-bezier(0.22, 1, 0.36, 1), transform 0.8s cubic-bezier(0.22, 1, 0.36, 1)';
        revealObserver.observe(el);
    });

    // Add revealed class style via JS to avoid extra CSS file if possible
    const style = document.createElement('style');
    style.innerHTML = `
        .revealed {
            opacity: 1 !important;
            transform: translateY(0) !important;
        }
    `;
    document.head.appendChild(style);
});
