import { cva, type VariantProps } from 'class-variance-authority'
import type { ComponentProps } from 'react'

import { cn } from '@/utils/ui'

const badgeVariants = cva(
  'inline-flex max-w-full items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium leading-relaxed [&_svg]:size-3 [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        default: 'border-transparent bg-primary text-primary-foreground',
        secondary: 'border-transparent bg-secondary text-secondary-foreground',
        outline: 'border-border bg-transparent text-muted-foreground',
        destructive: 'border-destructive/15 bg-destructive/10 text-destructive'
      }
    },
    defaultVariants: { variant: 'default' }
  }
)

function Badge({
  className,
  variant,
  ...props
}: ComponentProps<'span'> & VariantProps<typeof badgeVariants>) {
  return (
    <span className={cn(badgeVariants({ variant }), className)} {...props} />
  )
}

export { Badge, badgeVariants }
