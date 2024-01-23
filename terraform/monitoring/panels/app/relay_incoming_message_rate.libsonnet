local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Msg In Rate',
      datasource  = ds.prometheus,
    )
    .configure(
      defaults.configuration.timeseries
        .withUnit('cps')
    )

    .setAlert(vars.environment, grafana.alert.new(
      namespace     = vars.namespace,
      name          = '%(env)s - Not receiving any watch subscriptions requests' % { env: vars.environment },
      message       = '%(env)s - Not receiving any watch subscriptions requests' % { env: vars.environment },
      notifications = vars.notifications,
      noDataState   = 'no_data',
      period        = '30m',
      conditions    = [
        grafana.alertCondition.new(
          evaluatorParams = [ 1 ],
          evaluatorType   = 'lt',
          operatorType    = 'or',
          queryRefId      = 'RelayIncomingWatchSubscriptionsRate',
          queryTimeStart  = '5m',
          queryTimeEnd    = 'now',
          reducerType     = grafana.alert_reducers.Avg
        ),
      ],
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision, tag) (rate(relay_incoming_messages_total[$__rate_interval]))',
      legendFormat  = '{{tag}} r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'RelayIncomingMessagesRate',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum(increase(relay_incoming_messages_total{tag="4010"}[$__rate_interval]))',
      legendFormat  = '{{tag}} r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'RelayIncomingWatchSubscriptionsRate',
      hide          = true,
    ))
}
