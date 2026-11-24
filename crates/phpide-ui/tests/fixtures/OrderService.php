<?php

declare(strict_types=1);

namespace App\Controller;

use App\Repository\OrderRepository;
use Psr\Log\LoggerInterface;

/**
 * Places and cancels orders.
 *
 * @author Shop Team
 */
#[Service(name: 'orders')]
final class OrderService implements ServiceInterface
{
    private const MAX_ITEMS = 50;

    public function __construct(
        private readonly OrderRepository $orders,
        private readonly LoggerInterface $logger,
    ) {
    }

    /**
     * @param array<int, array{sku: string, qty: int}> $items
     */
    public function place(int $customerId, array $items): Order
    {
        if (count($items) > self::MAX_ITEMS) {
            throw new \InvalidArgumentException("Too many items: " . count($items));
        }

        $order = new Order($customerId);
        foreach ($items as $item) {
            $order->add($item['sku'], $item['qty']);
        }

        $this->orders->save($order);
        $this->logger->info('order placed', ['id' => $order->id]);

        return $order;
    }

    public function cancel(Order $order, ?string $reason = null): void
    {
        match ($order->status) {
            Status::Shipped => throw new \LogicException('already shipped'),
            Status::Cancelled => null,
            default => $order->cancel($reason ?? 'customer request'),
        };
        $this->orders->save($order);
    }

    // Totals are cached per request.
    private function total(Order $order): float
    {
        return array_sum(array_map(fn ($line) => $line->price * $line->qty, $order->lines));
    }
}
